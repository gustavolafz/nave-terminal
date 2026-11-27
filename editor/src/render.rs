use crate::config::EditorConfig;
use crate::cp_assist::{detect_cp_pitfall, file_has_custom_hash, CompletionKind, CompletionPopup};
use crate::document::{Document, Selection};
use crate::git::GutterMark;
use crate::judge::{DiagSeverity, Diagnostic};
use crate::layout::{ComputedLayout, DividerHit, PaneId, Rect, SplitAxis};
use crate::palette::{lerp_grad, push_bg, push_fg, push_fg_styled, push_grad_text, Palette, Rgb, TokenKind};
use crate::picker::{Picker, PickerAction};
use crate::pty_term::{render_terminal_cells, VtScreen};
use crate::syntax::{find_matching_bracket, Span};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::Instant;

const SYNC_START: &str = "\x1b[?2026h";
const SYNC_END: &str = "\x1b[?2026l";
const RST: &str = "\x1b[0m";

#[inline]
fn push_base_bg(out: &mut String, cfg: &EditorConfig, pal: &Palette) {
    if cfg.opaque_bg {
        push_bg(out, pal.bg);
    } else {
        out.push_str("\x1b[49m");
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VisualRowInfo {
    pub doc_row: usize,
    pub start_char_col: usize,
    pub end_char_col: usize,
}

#[derive(Clone, Debug, Default)]
pub struct PaneLayoutInfo {
    pub pane_id: PaneId,
    pub rect: Rect,
    pub gutter_width: usize,
    pub content_start_row: usize,
    pub content_height: usize,
    pub scrollbar_col: Option<usize>,
    pub visual_rows: Vec<VisualRowInfo>,
    pub tab_bar_row: usize,
    pub tab_hitboxes: Vec<(usize, usize, usize)>, // (start_col, end_col, tab_idx)
}

#[derive(Clone, Debug, Default)]
pub struct LayoutInfo {
    pub panes: Vec<PaneLayoutInfo>,
    pub dividers: Vec<DividerHit>,
    pub primary_cursor_screen: Option<(usize, usize)>,
}

/// Fast comet/smear travel trail drawn along the exact path of a cursor jump
/// (Ctrl+Arrows, Ctrl+1..9 pane jump, mouse click, Goto Line, Ctrl+], F8, etc.).
#[derive(Clone, Debug)]
pub struct CursorTrail {
    pub from: (usize, usize),
    pub to: (usize, usize),
    pub path: Vec<(usize, usize)>, // (abs_row, abs_col) spine from start -> end
    pub started_at: Instant,
    pub duration_ms: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct TrailCell {
    pub bg: Rgb,
    pub intensity: f32,
    pub is_hot: bool,
    pub dust: Option<(char, Rgb)>, // delicate stardust mote drawn ONLY on blank space cells
}

#[inline]
fn smootherstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    // Quintic C2-continuous S-curve: f(0)=0, f'(0)=0, f''(0)=0, f(1)=1, f'(1)=0, f''(1)=0
    // Finite acceleration on launch & velvet deceleration on landing (zero infinite jerk!)
    x * x * x * (x * (x * 6.0 - 15.0) + 10.0)
}

impl CursorTrail {
    pub fn new(from: (usize, usize), to: (usize, usize), now: Instant) -> Option<Self> {
        let dr = to.0 as f32 - from.0 as f32;
        let dc = to.1 as f32 - from.1 as f32;
        // Terminal cells are ~2:1 tall, so scale row delta by 2.0 for perceptual distance
        let dist = ((dr * 2.0) * (dr * 2.0) + dc * dc).sqrt();
        if dist < 2.0 {
            return None;
        }
        let steps = ((dr.abs() * 2.0).max(dc.abs()) * 2.0).ceil().max(2.0) as usize;
        let mut path = Vec::with_capacity(steps + 1);
        for i in 0..=steps {
            let u = i as f32 / steps as f32;
            let r = (from.0 as f32 + dr * u).round().max(0.0) as usize;
            let c = (from.1 as f32 + dc * u).round().max(0.0) as usize;
            if path.last().copied() != Some((r, c)) {
                path.push((r, c));
            }
        }
        if path.len() < 2 {
            return None;
        }
        // Smooth shooting-star glide: ~140ms word jump up to ~238ms cross-pane meteor
        let duration_ms = (122.0 + dist * 1.55).clamp(138.0, 238.0);
        Some(Self {
            from,
            to,
            path,
            started_at: now,
            duration_ms,
        })
    }

    pub fn progress(&self, now: Instant) -> f32 {
        (now.saturating_duration_since(self.started_at).as_secs_f32() * 1000.0)
            / self.duration_ms.max(1.0)
    }

    pub fn head_u(&self, now: Instant) -> f32 {
        let t = self.progress(now);
        smootherstep(t / 0.72)
    }

    pub fn current_head_pos(&self, now: Instant) -> Option<(usize, usize)> {
        if self.path.is_empty() {
            return None;
        }
        let idx = ((self.head_u(now) * (self.path.len() - 1) as f32).round() as usize)
            .min(self.path.len() - 1);
        Some(self.path[idx])
    }

    pub fn is_head_flying(&self, now: Instant) -> bool {
        self.progress(now) < 0.66
    }

    pub fn is_finished(&self, now: Instant) -> bool {
        self.progress(now) >= 1.0
    }
}

#[inline]
fn lerp_rgb(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| -> u8 {
        ((x as f32) + ((y as f32) - (x as f32)) * t).round().clamp(0.0, 255.0) as u8
    };
    (mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

pub fn build_trail_map(
    trails: &[CursorTrail],
    now: Instant,
    pal: &Palette,
) -> HashMap<(usize, usize), TrailCell> {
    let mut map: HashMap<(usize, usize), TrailCell> = HashMap::new();
    for tr in trails {
        let t = tr.progress(now);
        if t >= 1.0 || tr.path.len() < 2 {
            continue;
        }
        // Two phase-shifted C2 smootherstep curves (finite acceleration on launch & smooth landing)
        let head_u = smootherstep(t / 0.72);
        let tail_u = smootherstep((t - 0.10) / 0.90);
        let fade_out = (1.0 - t * t * t).clamp(0.0, 1.0);

        let dr = tr.to.0 as f32 - tr.from.0 as f32;
        let dc = tr.to.1 as f32 - tr.from.1 as f32;
        let dy = dr * 2.0; // perceptual aspect-corrected Y (cells are ~2:1 tall)
        let dx = dc;
        let total_len = (dx * dx + dy * dy).sqrt().max(1.0);
        let tx = dx / total_len;
        let ty = dy / total_len;
        let nx = -ty;
        let ny = tx;

        // Diagonal factor: 0.0 for pure horizontal/vertical (razor 1-cell streak),
        // ramps smoothly to 1.0 for diagonal jumps -> 2D conical shooting-star plume!
        let diag_factor = if dr.abs() >= 1.0 && dc.abs() >= 2.0 {
            ((dr.abs() * 2.0).min(dc.abs()) / 3.5).clamp(0.35, 1.0)
        } else {
            0.0
        };

        let p0x = tr.from.1 as f32;
        let p0y = tr.from.0 as f32 * 2.0;
        let ax = p0x + dx * tail_u;
        let ay = p0y + dy * tail_u;
        let bx = p0x + dx * head_u;
        let by = p0y + dy * head_u;
        let seg_len = ((bx - ax) * (bx - ax) + (by - ay) * (by - ay)).sqrt();
        let head_r = (by * 0.5).round().max(0.0) as usize;
        let head_c = bx.round().max(0.0) as usize;

        let mut insert_cell = |r: usize, c: usize, cell: TrailCell| {
            map.entry((r, c))
                .and_modify(|prev| {
                    if cell.intensity > prev.intensity {
                        *prev = cell;
                    }
                })
                .or_insert(cell);
        };

        if diag_factor > 0.0 && seg_len >= 1.2 {
            // 2D Conical Shooting-Star Plume:
            // Pinpoint incandescent nose at B (rel=1.0) -> smoothly widening conical aura toward A (rel=0.0)
            let max_cone_r = (1.8 + (seg_len * 0.095).min(2.1)) * diag_factor;
            let pad_c = max_cone_r.ceil() as isize + 1;
            let pad_r = (max_cone_r * 0.5).ceil() as isize + 1;

            let min_r = ((ay.min(by) * 0.5).floor() as isize - pad_r).max(0) as usize;
            let max_r = ((ay.max(by) * 0.5).ceil() as isize + pad_r).max(0) as usize;
            let min_c = (ax.min(bx).floor() as isize - pad_c).max(0) as usize;
            let max_c = (ax.max(bx).ceil() as isize + pad_c).max(0) as usize;

            for r in min_r..=max_r {
                let qy = r as f32 * 2.0;
                for c in min_c..=max_c {
                    let qx = c as f32;
                    let is_head_cell = r == head_r && c == head_c;

                    let vx = qx - ax;
                    let vy = qy - ay;
                    let proj = vx * tx + vy * ty;
                    let perp = (vx * nx + vy * ny).abs();

                    if !is_head_cell && (proj < -0.5 || proj > seg_len + 0.55) {
                        continue;
                    }
                    let rel = if is_head_cell {
                        1.0
                    } else {
                        (proj / seg_len).clamp(0.0, 1.0)
                    };

                    // Conical half-width: sharp pinpoint at tip (0.62), smoothly flares out behind
                    let cone_r = 0.62 + max_cone_r * (1.0 - rel).powf(0.82);
                    if !is_head_cell && perp > cone_r {
                        continue;
                    }

                    // Smooth radial Gaussian-like falloff across cone cross-section (soft edges!)
                    let norm_p = (perp / cone_r).clamp(0.0, 1.0);
                    let radial = (1.0 - norm_p * norm_p).powf(1.55);
                    // Longitudinal meteor taper (brightest near tip, dissipating gracefully at rear)
                    let longi = rel.powf(1.45);
                    let intensity = if is_head_cell {
                        (0.80 + 0.20 * (1.0 - t)).min(1.0) * fade_out
                    } else {
                        (0.06 + 0.94 * longi) * radial * fade_out
                    };
                    if intensity < 0.035 {
                        continue;
                    }

                    // Color flows from white-cyan core along spine -> violet-magenta along cone wings!
                    let grad_pos = (1.0 - rel * 0.72 + norm_p * 0.36).clamp(0.0, 1.0);
                    let base_neon = lerp_grad(pal.grad, grad_pos);
                    let head_white = if is_head_cell {
                        0.85 * fade_out
                    } else {
                        ((rel - 0.76) / 0.24).clamp(0.0, 1.0) * radial * 0.65 * fade_out
                    };
                    let star_color = lerp_rgb(base_neon, (255, 255, 255), head_white);
                    let bg_alpha = if is_head_cell {
                        (0.88 * fade_out).clamp(0.0, 0.94)
                    } else {
                        (intensity * 0.76 + head_white * 0.14).clamp(0.0, 0.88)
                    };
                    let bg = lerp_rgb(pal.bg, star_color, bg_alpha);

                    // Delicate stardust sparks along the conical wake & outer fringes (blank spaces only)
                    let dust = if !is_head_cell && rel < 0.82 && intensity > 0.04 && intensity < 0.58 {
                        let h = (r.wrapping_mul(37) ^ c.wrapping_mul(19) ^ ((rel * 11.0) as usize)) % 7;
                        if norm_p < 0.55 && rel > 0.35 && h <= 2 {
                            Some(('∙', lerp_rgb(pal.bg, star_color, (0.48 + intensity * 0.50).min(1.0))))
                        } else if h <= 3 {
                            Some(('·', lerp_rgb(pal.bg, base_neon, (0.30 + intensity * 0.60).min(0.95))))
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    insert_cell(
                        r,
                        c,
                        TrailCell {
                            bg,
                            intensity,
                            is_hot: is_head_cell || (intensity > 0.56 && norm_p < 0.45),
                            dust,
                        },
                    );
                }
            }
        } else {
            // Pure horizontal/vertical jump (e.g. Ctrl+Right word travel): razor-sharp 1-line shooting star
            let denom = (tr.path.len() - 1) as f32;
            let head_idx = ((head_u * denom).round() as usize).min(tr.path.len() - 1);
            let span = (head_u - tail_u).max(0.04);

            for (idx, &(r, c)) in tr.path.iter().enumerate() {
                let u = idx as f32 / denom;
                let is_head_cell = idx == head_idx;
                if !is_head_cell && (u < tail_u || u > head_u) {
                    continue;
                }
                let rel = if is_head_cell {
                    1.0
                } else {
                    ((u - tail_u) / span).clamp(0.0, 1.0)
                };
                let core = rel.powf(2.20);
                let intensity = if is_head_cell {
                    (0.75 + 0.25 * (1.0 - t)).min(1.0) * fade_out
                } else {
                    (0.08 + 0.92 * core) * fade_out
                };
                if intensity < 0.03 {
                    continue;
                }
                let base_neon = lerp_grad(pal.grad, 1.0 - rel);
                let head_white = if is_head_cell {
                    0.82 * fade_out
                } else {
                    ((rel - 0.78) / 0.22).clamp(0.0, 1.0) * 0.65 * fade_out
                };
                let star_color = lerp_rgb(base_neon, (255, 255, 255), head_white);
                let bg_alpha = if is_head_cell {
                    (0.86 * fade_out).clamp(0.0, 0.94)
                } else {
                    (intensity * 0.74 + head_white * 0.14).clamp(0.0, 0.88)
                };
                let bg = lerp_rgb(pal.bg, star_color, bg_alpha);

                let dust = if !is_head_cell && rel > 0.08 && rel < 0.78 && intensity > 0.04 {
                    let h = (r.wrapping_mul(31) ^ c.wrapping_mul(17) ^ idx.wrapping_mul(13)) % 5;
                    if rel > 0.46 && h <= 2 {
                        Some(('∙', lerp_rgb(pal.bg, star_color, (0.55 + intensity * 0.45).min(1.0))))
                    } else if h <= 3 {
                        Some(('·', lerp_rgb(pal.bg, base_neon, (0.35 + intensity * 0.55).min(0.95))))
                    } else {
                        None
                    }
                } else {
                    None
                };

                insert_cell(
                    r,
                    c,
                    TrailCell {
                        bg,
                        intensity,
                        is_hot: is_head_cell || intensity > 0.54 || head_white > 0.35,
                        dust,
                    },
                );
            }
        }
    }
    map
}

/// Predicts the focused pane's primary cursor `(abs_row, abs_col)` upfront so cursor trail
/// animations launch on frame 0 with zero latency.
pub fn predict_primary_screen_pos(
    doc: &Document,
    p: &PaneRenderInput,
    cfg: &EditorConfig,
) -> Option<(usize, usize)> {
    let rect = p.rect;
    let total_lines = doc.rope.line_count();
    let content_h = rect.h.saturating_sub(1);
    let content_start_row = rect.y + 1;
    let raw_gutter_w = compute_gutter_width(total_lines);
    let show_gutter = rect.w >= raw_gutter_w + 4;
    let gutter_w = if show_gutter { raw_gutter_w } else { 0 };
    let show_sb = cfg.scrollbar && rect.w >= gutter_w + 16;
    let sb_w = if show_sb { 1 } else { 0 };
    let text_w = rect.w.saturating_sub(gutter_w + sb_w);
    if text_w == 0 || content_h == 0 {
        return None;
    }

    let primary_sel = p
        .cursors
        .get(p.primary.min(p.cursors.len().saturating_sub(1)))
        .copied()
        .unwrap_or_else(|| Selection::caret(0));
    let head = doc.clamp_byte_pos(primary_sel.head);
    let (prow, pcol) = doc.rope.byte_to_line_col(head);
    if prow < p.scroll_row {
        return None;
    }

    if !cfg.wrap {
        let sy = prow - p.scroll_row;
        if sy >= content_h {
            return None;
        }
        let line_txt = doc.rope.line(prow);
        let vcol = visual_col_of(&line_txt, pcol, doc.tab_width);
        if vcol >= p.scroll_col && vcol - p.scroll_col < text_w {
            return Some((content_start_row + sy, rect.x + gutter_w + (vcol - p.scroll_col)));
        }
        return None;
    }

    // Wrapped mode: count exact wrapped visual rows from scroll_row to prow
    let mut sy = 0usize;
    for r in p.scroll_row..=prow.min(total_lines.saturating_sub(1)) {
        if sy >= content_h {
            return None;
        }
        let line_txt = doc.rope.line(r);
        let ls = doc.rope.line_start_byte(r);
        let cells = build_char_cells(&line_txt, ls, &[], doc.tab_width);
        let segs = split_wrapped_segments(&cells, text_w);
        if r == prow {
            for (seg_start, seg_end) in segs {
                if sy >= content_h {
                    return None;
                }
                if (pcol >= seg_start && pcol < seg_end)
                    || (seg_end == cells.len() && pcol == seg_end)
                {
                    let c_vis: usize = cells[seg_start..pcol.min(seg_end)]
                        .iter()
                        .map(|c| c.width)
                        .sum();
                    if c_vis < text_w {
                        return Some((content_start_row + sy, rect.x + gutter_w + c_vis));
                    }
                    return None;
                }
                sy += 1;
            }
            return None;
        } else {
            sy += segs.len();
        }
    }
    None
}

#[derive(Clone, Debug)]
pub struct TabBadge {
    pub name: String,
    pub dirty: bool,
    pub active: bool,
}

#[derive(Clone, Debug)]
pub struct PaneRenderInput {
    pub pane_id: PaneId,
    pub group_num: usize, // 1-based group index
    pub rect: Rect,
    pub is_focused: bool,
    pub doc_id: usize,
    pub tabs: Vec<TabBadge>,
    pub scroll_row: usize,
    pub scroll_col: usize,
    pub cursors: Vec<Selection>,
    pub primary: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropZone {
    TabBar(usize), // insertion index inside target pane's tab list
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Debug)]
pub struct DropPreview {
    pub target_pane: PaneId,
    pub zone: DropZone,
    pub preview_rect: Rect,
    pub tab_insert_col: Option<usize>,
    pub file_name: String,
    pub label: &'static str,
}

pub struct TermPanelView<'a> {
    pub screen: &'a VtScreen,
    pub header_row: usize,
    pub body_height: usize,
    pub is_focused: bool,
    pub hover_divider: bool,
    pub dragging_divider: bool,
    pub hover_close: bool,
    pub cwd_label: &'a str,
    pub shell_name: &'a str,
}

pub enum PromptBar<'a> {
    None,
    Find {
        query: &'a str,
        count: usize,
        active: Option<usize>,
        case: bool,
        word: bool,
        regex: bool,
        error: Option<&'a str>,
    },
    Replace {
        query: &'a str,
        replace: &'a str,
        focus_replace: bool,
        count: usize,
        active: Option<usize>,
        case: bool,
        word: bool,
        regex: bool,
        error: Option<&'a str>,
    },
    Input {
        label: &'a str,
        value: &'a str,
        hint: &'a str,
    },
    Confirm {
        message: &'a str,
    },
}

pub fn char_display_width(ch: char) -> usize {
    let cp = ch as u32;
    if cp == 0 || (0x0300..=0x036F).contains(&cp) || (0xFE00..=0xFE0F).contains(&cp) {
        return 0;
    }
    if cp < 0x1100 {
        return 1;
    }
    if (0x1100..=0x115F).contains(&cp)
        || (0x2329..=0x232A).contains(&cp)
        || (0x2E80..=0xA4CF).contains(&cp)
        || (0xAC00..=0xD7A3).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0xFE10..=0xFE6F).contains(&cp)
        || (0xFF00..=0xFF60).contains(&cp)
        || (0xFFE0..=0xFFE6).contains(&cp)
        || (0x1F300..=0x1FAFF).contains(&cp)
    {
        2
    } else {
        1
    }
}

pub fn visual_col_of(line: &str, target_char_col: usize, tab_w: usize) -> usize {
    let mut v = 0usize;
    for (i, ch) in line.chars().enumerate() {
        if i == target_char_col {
            break;
        }
        if ch == '\t' {
            v += tab_w - (v % tab_w);
        } else {
            v += char_display_width(ch);
        }
    }
    v
}

pub fn char_col_at_visual(line: &str, target_vis: usize, tab_w: usize) -> usize {
    let mut v = 0usize;
    for (i, ch) in line.chars().enumerate() {
        let w = if ch == '\t' {
            tab_w - (v % tab_w)
        } else {
            char_display_width(ch)
        };
        if v + w > target_vis {
            return i;
        }
        v += w;
    }
    line.chars().count()
}

pub fn ensure_cursor_visible(
    doc: &mut Document,
    pane_w: usize,
    pane_content_h: usize,
    cfg: &EditorConfig,
) {
    let view_h = pane_content_h.max(1);
    let gutter_w = compute_gutter_width(doc.rope.line_count());
    let sb_w = if cfg.scrollbar && pane_w >= 24 { 1 } else { 0 };
    let text_w = pane_w.saturating_sub(gutter_w + sb_w).max(4);
    let (prow, pcol) = doc.primary_line_col();

    if cfg.center_scroll {
        doc.scroll_row = prow.saturating_sub(view_h / 2);
    } else {
        let scrolloff = cfg.scrolloff.min(view_h / 2);
        if prow < doc.scroll_row + scrolloff {
            doc.scroll_row = prow.saturating_sub(scrolloff);
        } else if prow + scrolloff >= doc.scroll_row + view_h {
            doc.scroll_row = (prow + scrolloff + 1).saturating_sub(view_h);
        }
    }

    if cfg.wrap {
        doc.scroll_col = 0;
    } else {
        let line_txt = doc.rope.line(prow);
        let vcol = visual_col_of(&line_txt, pcol, doc.tab_width);
        let h_margin = 4.min(text_w / 4);
        if vcol < doc.scroll_col + h_margin {
            doc.scroll_col = vcol.saturating_sub(h_margin);
        } else if vcol + h_margin >= doc.scroll_col + text_w {
            doc.scroll_col = (vcol + h_margin + 1).saturating_sub(text_w);
        }
    }
}

pub fn compute_gutter_width(line_count: usize) -> usize {
    let digits = line_count.max(1).to_string().len().max(3);
    digits + 5 // "▸ 123 ▎ " or "  123 ▎ "
}

fn group_badge_glyph(num: usize) -> String {
    match num {
        1 => "①".into(),
        2 => "②".into(),
        3 => "③".into(),
        4 => "④".into(),
        5 => "⑤".into(),
        6 => "⑥".into(),
        7 => "⑦".into(),
        8 => "⑧".into(),
        9 => "⑨".into(),
        n => format!("[{}]", n),
    }
}

pub fn render_editor(
    docs: &mut [Option<Document>],
    pane_views: &[PaneRenderInput],
    computed: &ComputedLayout,
    active_divider: Option<usize>,
    hover_divider: Option<usize>,
    hot_scrollbar: Option<PaneId>,
    highlight_all_dividers: bool,
    drop_preview: Option<&DropPreview>,
    term_panel: Option<&TermPanelView<'_>>,
    term_bg_alive: bool,
    completion: Option<&CompletionPopup>,
    active_diags: &[Diagnostic],
    timer_badge: Option<&(String, bool)>,
    judge_progress: Option<&str>,
    trail_cells: &HashMap<(usize, usize), TrailCell>,
    hide_editor_cursor: bool,
    pal: &Palette,
    cols: usize,
    rows: usize,
    cfg: &EditorConfig,
    prompt: PromptBar<'_>,
    picker: Option<&Picker>,
    status_msg: Option<&str>,
    mode_hint: Option<&str>,
    synced: bool,
) -> (String, LayoutInfo) {
    let mut out = String::with_capacity(cols * rows * 4);
    if synced {
        out.push_str(SYNC_START);
    }
    out.push_str("\x1b[?25l");

    let has_prompt = !matches!(prompt, PromptBar::None);
    let multi_pane = pane_views.len() > 1;

    let mut layout = LayoutInfo {
        panes: Vec::with_capacity(pane_views.len()),
        dividers: computed.dividers.clone(),
        primary_cursor_screen: None,
    };

    let mut primary_screen_pos: Option<(usize, usize)> = None;
    let mut active_status_info: Option<(usize, usize, usize, usize, String, String, bool, usize)> = None;

    // -------------------------------------------------------------------------
    // Render each Pane inside its exact Rect (header tab bar + editor canvas)
    // -------------------------------------------------------------------------
    for p in pane_views {
        let Some(doc) = docs.get_mut(p.doc_id).and_then(|d| d.as_mut()) else {
            continue;
        };

        let sb_hot = hot_scrollbar == Some(p.pane_id);
        let pane_diags: &[Diagnostic] = if p.is_focused { active_diags } else { &[] };
        let pane_ghost: Option<&str> = if p.is_focused {
            completion.and_then(|c| c.ghost_suffix())
        } else {
            None
        };
        let (p_layout, p_cursor, sel_chars, cur_row, cur_col, cur_count) = render_pane(
            &mut out,
            doc,
            p,
            multi_pane,
            sb_hot,
            pane_diags,
            pane_ghost,
            trail_cells,
            pal,
            cfg,
        );

        if p.is_focused {
            primary_screen_pos = p_cursor;
            layout.primary_cursor_screen = p_cursor;
            active_status_info = Some((
                cur_row,
                cur_col,
                sel_chars,
                cur_count,
                doc.lang.name().to_string(),
                doc.eol.label().to_string(),
                doc.external_changed,
                p.group_num,
            ));
        }

        layout.panes.push(p_layout);
    }

    // -------------------------------------------------------------------------
    // Render Split Dividers (`│`, `─`) and Intersections (`┼`, `├`, `┤`, `┬`, `┴`)
    // -------------------------------------------------------------------------
    if multi_pane {
        push_base_bg(&mut out, cfg, pal);
        for d in &computed.dividers {
            let is_drag = highlight_all_dividers || active_divider == Some(d.split_id);
            let is_hover = hover_divider == Some(d.split_id);
            let div_fg = if is_drag || is_hover { pal.grad[0] } else { pal.dim };
            let div_bold = is_drag || is_hover;
            match d.axis {
                SplitAxis::Horizontal => {
                    let glyph = if is_drag { '┃' } else { '│' };
                    for r in d.y..(d.y + d.len) {
                        if let Some(tc) = trail_cells.get(&(r, d.x)) {
                            push_bg(&mut out, tc.bg);
                            push_fg(&mut out, if tc.is_hot { pal.bracket_fg } else { pal.grad[0] }, true);
                        } else {
                            push_base_bg(&mut out, cfg, pal);
                            push_fg(&mut out, div_fg, div_bold);
                        }
                        let _ = write!(out, "\x1b[{};{}H{}", r + 1, d.x + 1, glyph);
                    }
                }
                SplitAxis::Vertical => {
                    let glyph = if is_drag { '━' } else { '─' };
                    let _ = write!(out, "\x1b[{};{}H", d.y + 1, d.x + 1);
                    for off in 0..d.len {
                        let c = d.x + off;
                        if let Some(tc) = trail_cells.get(&(d.y, c)) {
                            push_bg(&mut out, tc.bg);
                            push_fg(&mut out, if tc.is_hot { pal.bracket_fg } else { pal.grad[0] }, true);
                        } else {
                            push_base_bg(&mut out, cfg, pal);
                            push_fg(&mut out, div_fg, div_bold);
                        }
                        out.push(glyph);
                    }
                }
            }
        }
        if !computed.junctions.is_empty() {
            let j_hot = highlight_all_dividers || active_divider.is_some() || hover_divider.is_some();
            let j_fg = if j_hot { pal.grad[0] } else { pal.dim };
            for &(jx, jy, jch) in &computed.junctions {
                if let Some(tc) = trail_cells.get(&(jy, jx)) {
                    push_bg(&mut out, tc.bg);
                    push_fg(&mut out, if tc.is_hot { pal.bracket_fg } else { pal.grad[0] }, true);
                } else {
                    push_base_bg(&mut out, cfg, pal);
                    push_fg(&mut out, j_fg, j_hot);
                }
                let _ = write!(out, "\x1b[{};{}H{}", jy + 1, jx + 1, jch);
            }
        }
        out.push_str(RST);
    }

    // -------------------------------------------------------------------------
    // Live Tab Drag-and-Drop Zone Preview Overlay (VS Code style!)
    // -------------------------------------------------------------------------
    if let Some(dp) = drop_preview {
        match dp.zone {
            DropZone::TabBar(_) => {
                if let Some(ins_col) = dp.tab_insert_col {
                    if let Some(pl) = layout.panes.iter().find(|p| p.pane_id == dp.target_pane) {
                        let cx = ins_col.min(cols.saturating_sub(1));
                        let _ = write!(out, "\x1b[{};{}H", pl.tab_bar_row + 1, cx + 1);
                        push_bg(&mut out, pal.grad[0]);
                        push_fg(&mut out, pal.bar_bg, true);
                        out.push('┃');
                        out.push_str(RST);
                    }
                }
            }
            DropZone::Center | DropZone::Left | DropZone::Right | DropZone::Top | DropZone::Bottom => {
                let r = dp.preview_rect;
                if r.w >= 4 && r.h >= 3 {
                    // Draw glowing rounded preview frame (`╭──╮`, `│`, `╰──╯`)
                    push_bg(&mut out, pal.sel_bg);
                    push_fg(&mut out, pal.grad[0], true);

                    // Top edge
                    let _ = write!(out, "\x1b[{};{}H╭", r.y + 1, r.x + 1);
                    for _ in 0..r.w.saturating_sub(2) {
                        out.push('─');
                    }
                    out.push('╮');

                    // Side edges
                    for dy in 1..r.h.saturating_sub(1) {
                        let y = r.y + dy + 1;
                        let _ = write!(out, "\x1b[{};{}H│", y, r.x + 1);
                        let _ = write!(out, "\x1b[{};{}H│", y, r.x + r.w);
                    }

                    // Bottom edge
                    let _ = write!(out, "\x1b[{};{}H╰", r.y + r.h, r.x + 1);
                    for _ in 0..r.w.saturating_sub(2) {
                        out.push('─');
                    }
                    out.push('╯');

                    // Centered floating pill label showing filename + destination action
                    let badge_raw = format!(" ⬚ {} → {} ", dp.file_name, dp.label);
                    let max_bw = r.w.saturating_sub(2);
                    if max_bw >= 6 {
                        let badge = truncate_to_cols(&badge_raw, max_bw);
                        let bw = str_display_width(&badge);
                        let bx = r.x + 1 + (max_bw.saturating_sub(bw)) / 2;
                        let by = r.y + r.h / 2;
                        let _ = write!(out, "\x1b[{};{}H", by + 1, bx + 1);
                        push_bg(&mut out, pal.grad[0]);
                        push_fg(&mut out, pal.bar_bg, true);
                        out.push_str(&badge);
                    }
                    out.push_str(RST);
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // Integrated Bottom Terminal Drawer (VS Code Ctrl+' style)
    // -------------------------------------------------------------------------
    let mut term_cursor_pos: Option<(usize, usize)> = None;
    if let Some(tp) = term_panel {
        // 1-row draggable horizontal divider + terminal header bar
        let _ = write!(out, "\x1b[{};1H", tp.header_row + 1);
        let bar_bg = if tp.is_focused {
            pal.tab_active_bg
        } else {
            pal.bar_bg
        };
        push_bg(&mut out, bar_bg);

        let is_hot = tp.hover_divider || tp.dragging_divider;
        let line_ch = if tp.dragging_divider { '━' } else { '─' };
        let mut used = 0usize;

        let scroll_badge = if tp.screen.scroll_offset > 0 {
            format!(" [SCROLL -{}] ", tp.screen.scroll_offset)
        } else {
            String::new()
        };
        let title_extra = if !tp.screen.title.is_empty() {
            tp.screen.title.as_str()
        } else {
            tp.cwd_label
        };
        let label = format!(" ❯ TERMINAL ({}) · {} ", tp.shell_name, title_extra);
        let close_btn = " × ";
        let right_hint = if tp.is_focused {
            "Ctrl+' ocultar · Ctrl+1..9 editor "
        } else {
            "Ctrl+' focar · arraste ↕ "
        };

        let close_w = str_display_width(close_btn);
        let avail_before_close = cols.saturating_sub(close_w);

        // Leading grip `──`
        if avail_before_close >= 2 {
            push_fg(
                &mut out,
                if is_hot || tp.is_focused { pal.grad[0] } else { pal.dim },
                is_hot || tp.is_focused,
            );
            out.push(line_ch);
            out.push(line_ch);
            used += 2;
        }

        // Terminal title pill
        let lbl_disp = truncate_to_cols(&label, avail_before_close.saturating_sub(used));
        if tp.is_focused {
            push_grad_text(&mut out, &lbl_disp, pal.grad, true);
        } else {
            push_fg(&mut out, pal.fg, false);
            out.push_str(&lbl_disp);
        }
        used += str_display_width(&lbl_disp);

        // Optional scrollback badge
        if !scroll_badge.is_empty() && used + str_display_width(&scroll_badge) <= avail_before_close {
            push_fg(&mut out, pal.warn, true);
            out.push_str(&scroll_badge);
            used += str_display_width(&scroll_badge);
        }

        // Fill middle horizontal rule (acts as drag handle) + right hint
        let hint_w = str_display_width(right_hint);
        let show_hint = used + hint_w + 3 <= avail_before_close;
        let fill_end = if show_hint {
            avail_before_close - hint_w
        } else {
            avail_before_close
        };
        push_fg(
            &mut out,
            if is_hot { pal.grad[0] } else if tp.is_focused { pal.border } else { pal.dim },
            is_hot,
        );
        while used < fill_end {
            out.push(line_ch);
            used += 1;
        }
        if show_hint {
            push_fg(&mut out, if is_hot { pal.grad[0] } else { pal.dim }, is_hot);
            out.push_str(right_hint);
            used += hint_w;
        }

        // Rightmost `[×]` close button
        if cols >= close_w {
            while used < cols - close_w {
                out.push(' ');
                used += 1;
            }
            if tp.hover_close {
                push_bg(&mut out, pal.err);
                push_fg(&mut out, pal.fg, true);
            } else {
                push_fg(&mut out, pal.dim, false);
            }
            out.push_str(close_btn);
        }
        out.push_str(RST);

        // Live terminal cells below header row
        term_cursor_pos = render_terminal_cells(
            &mut out,
            tp.screen,
            tp.header_row + 1,
            cols,
            tp.body_height,
            tp.is_focused,
            pal,
            if cfg.opaque_bg { Some(pal.bg) } else { None },
        );
    }

    // -------------------------------------------------------------------------
    // Optional Prompt Bar (Row `rows - 2`, 0-based)
    // -------------------------------------------------------------------------
    let mut prompt_cursor_pos: Option<(usize, usize)> = None;
    let prompt_row = rows.saturating_sub(2);

    if has_prompt {
        let _ = write!(out, "\x1b[{};1H", prompt_row + 1);
    }
    match prompt {
        PromptBar::None => {}
        PromptBar::Find {
            query,
            count,
            active,
            case,
            word,
            regex,
            error,
        } => {
            push_bg(&mut out, pal.tab_active_bg);
            let prefix = " Buscar: ";
            push_fg(&mut out, pal.grad[0], true);
            out.push_str(prefix);
            push_fg(&mut out, pal.fg, false);
            out.push_str(query);
            let cur_x = prefix.chars().count() + query.chars().count();
            prompt_cursor_pos = Some((prompt_row, cur_x.min(cols.saturating_sub(1))));

            let info = if let Some(err) = error {
                format!("  [{}]", err)
            } else if query.is_empty() {
                "  (Tab: substituir · Alt+C case · Alt+W palavra · Alt+R regex)".into()
            } else {
                let idx_s = active.map(|i| i + 1).unwrap_or(0);
                format!(
                    "  [{}/{}]  [{}Aa {}ab {}.*]  Enter/F3 prox · Tab subst",
                    idx_s,
                    count,
                    if case { "●" } else { "○" },
                    if word { "●" } else { "○" },
                    if regex { "●" } else { "○" }
                )
            };
            push_fg(&mut out, if error.is_some() { pal.err } else { pal.dim }, false);
            out.push_str(&truncate_to_cols(&info, cols.saturating_sub(cur_x)));
            out.push_str("\x1b[K");
            out.push_str(RST);
        }
        PromptBar::Replace {
            query,
            replace,
            focus_replace,
            count,
            active,
            case,
            word,
            regex,
            error,
        } => {
            push_bg(&mut out, pal.tab_active_bg);
            let p1 = " Buscar: ";
            let p2 = "  Substituir: ";
            push_fg(&mut out, pal.grad[0], !focus_replace);
            out.push_str(p1);
            push_fg(&mut out, pal.fg, false);
            out.push_str(query);
            let q_end = p1.chars().count() + query.chars().count();

            push_fg(&mut out, pal.grad[2], focus_replace);
            out.push_str(p2);
            push_fg(&mut out, pal.fg, false);
            out.push_str(replace);
            let r_end = q_end + p2.chars().count() + replace.chars().count();

            let cx = if focus_replace { r_end } else { q_end };
            prompt_cursor_pos = Some((prompt_row, cx.min(cols.saturating_sub(1))));

            let idx_s = active.map(|i| i + 1).unwrap_or(0);
            let info = if let Some(err) = error {
                format!("  [{}]", err)
            } else {
                format!(
                    "  [{}/{}] [{}{}{}] Enter:1 · Alt+A:todos · Tab:alternar",
                    idx_s,
                    count,
                    if case { "Aa " } else { "" },
                    if word { "ab " } else { "" },
                    if regex { ".*" } else { "" }
                )
            };
            push_fg(&mut out, if error.is_some() { pal.err } else { pal.dim }, false);
            out.push_str(&truncate_to_cols(&info, cols.saturating_sub(r_end)));
            out.push_str("\x1b[K");
            out.push_str(RST);
        }
        PromptBar::Input { label, value, hint } => {
            push_bg(&mut out, pal.tab_active_bg);
            let p = format!(" {}: ", label);
            push_fg(&mut out, pal.grad[0], true);
            out.push_str(&p);
            push_fg(&mut out, pal.fg, false);
            out.push_str(value);
            let cx = p.chars().count() + value.chars().count();
            prompt_cursor_pos = Some((prompt_row, cx.min(cols.saturating_sub(1))));
            push_fg(&mut out, pal.dim, false);
            let h = format!("  ({})", hint);
            out.push_str(&truncate_to_cols(&h, cols.saturating_sub(cx)));
            out.push_str("\x1b[K");
            out.push_str(RST);
        }
        PromptBar::Confirm { message } => {
            push_bg(&mut out, pal.tab_active_bg);
            push_fg(&mut out, pal.warn, true);
            out.push_str(&format!(" {} ", message));
            out.push_str("\x1b[K");
            out.push_str(RST);
        }
    }

    // -------------------------------------------------------------------------
    // Bottom Global Status Bar (Last Row)
    // -------------------------------------------------------------------------
    let _ = write!(out, "\x1b[{};1H", rows.max(1));
    push_bg(&mut out, pal.bar_bg);

    let (primary_row, primary_col, sel_chars, cur_count, lang_name, eol_lbl, ext_changed, active_group) =
        active_status_info.unwrap_or((0, 0, 0, 1, "text".into(), "LF".into(), false, 1));

    let multi_badge = if cur_count > 1 {
        format!(" · {} cursores", cur_count)
    } else if sel_chars > 0 {
        format!(" · Sel {}", sel_chars)
    } else {
        String::new()
    };
    let pane_badge = if multi_pane {
        format!(" · Painel {}/{}", active_group, pane_views.len())
    } else {
        String::new()
    };
    let left_status = if let Some(tp) = term_panel.filter(|t| t.is_focused) {
        if tp.screen.scroll_offset > 0 {
            format!(
                " ❯ TERMINAL [{}] · Scroll -{} linhas",
                tp.shell_name, tp.screen.scroll_offset
            )
        } else {
            format!(
                " ❯ TERMINAL [{}] · {},{}",
                tp.shell_name,
                tp.screen.cur_row + 1,
                tp.screen.cur_col + 1
            )
        }
    } else {
        format!(
            " Ln {}, Col {}{}{}",
            primary_row + 1,
            primary_col + 1,
            multi_badge,
            pane_badge
        )
    };
    push_fg(&mut out, pal.grad[0], true);
    let left_disp = truncate_to_cols(&left_status, cols);
    out.push_str(&left_disp);

    let mut used_s = str_display_width(&left_disp);
    if let Some((tb, expired)) = timer_badge {
        let t_str = format!("  │  {}", tb);
        push_fg(&mut out, if *expired { pal.err } else { pal.warn }, true);
        let disp = truncate_to_cols(&t_str, cols.saturating_sub(used_s));
        out.push_str(&disp);
        used_s += str_display_width(&disp);
    }
    if cfg.exam_mode {
        let em = " [PROVA]";
        if used_s + str_display_width(em) < cols {
            push_fg(&mut out, pal.err, true);
            out.push_str(em);
            used_s += str_display_width(em);
        }
    }
    if let Some(jp) = judge_progress {
        let m = format!("  │  ⚡ {}", jp);
        push_fg(&mut out, pal.grad[0], true);
        let disp = truncate_to_cols(&m, cols.saturating_sub(used_s));
        out.push_str(&disp);
        used_s += str_display_width(&disp);
    } else if let Some(hint) = mode_hint {
        let m = format!("  │  {}", hint);
        push_fg(&mut out, pal.warn, true);
        let disp = truncate_to_cols(&m, cols.saturating_sub(used_s));
        out.push_str(&disp);
        used_s += str_display_width(&disp);
    } else if let Some(msg) = status_msg {
        let m = format!("  │  {}", msg);
        push_fg(&mut out, pal.ok, false);
        let disp = truncate_to_cols(&m, cols.saturating_sub(used_s));
        out.push_str(&disp);
        used_s += str_display_width(&disp);
    } else if ext_changed {
        let m = "  │  ⚠ Alterado no disco (F9 recarregar)";
        push_fg(&mut out, pal.warn, true);
        let disp = truncate_to_cols(m, cols.saturating_sub(used_s));
        out.push_str(&disp);
        used_s += str_display_width(&disp);
    }

    let term_badge = if term_bg_alive && term_panel.is_none() {
        " [❯ term]"
    } else {
        ""
    };
    let right_help = if term_panel.is_some_and(|t| t.is_focused) {
        "Ctrl+' ocultar · Ctrl+1..9 editor · Shift+PgUp/Dn scroll · F1 cmds ".into()
    } else if multi_pane {
        format!(
            "[{} · {}{}{}] [{}]{} · Ctrl+' term · F1 cmds ",
            lang_name,
            eol_lbl,
            if cfg.wrap { " · WRAP" } else { "" },
            if cfg.relative_numbers { " · REL" } else { "" },
            pal.name,
            term_badge
        )
    } else {
        format!(
            "Ctrl+S salvar · Ctrl+\\ dividir · Ctrl+' term · Ctrl+P abrir{} · F1 cmds ",
            term_badge
        )
    };
    let rh_w = str_display_width(&right_help);
    if used_s + rh_w + 2 <= cols {
        let pad = cols - used_s - rh_w;
        for _ in 0..pad {
            out.push(' ');
        }
        push_fg(&mut out, pal.dim, false);
        out.push_str(&right_help);
    } else {
        out.push_str("\x1b[K");
    }
    out.push_str(RST);

    // -------------------------------------------------------------------------
    // Floating Modal Overlay for Quick Open (Ctrl+P) or Command Palette (F1)
    // -------------------------------------------------------------------------
    if let Some(pk) = picker {
        let modal_w = cols.saturating_sub(8).clamp(40, 76).min(cols);
        let max_items = rows.saturating_sub(8).clamp(4, 12);
        let modal_h = max_items + 4;
        let x0 = (cols.saturating_sub(modal_w)) / 2;
        let y0 = 2.min(rows.saturating_sub(modal_h));

        // Top border
        let _ = write!(out, "\x1b[{};{}H", y0 + 1, x0 + 1);
        push_bg(&mut out, pal.bar_bg);
        let max_title_w = modal_w.saturating_sub(6);
        let title_trimmed = truncate_to_cols(&pk.title, max_title_w);
        let title_str = format!(" {} ", title_trimmed);
        let fill = modal_w.saturating_sub(str_display_width(&title_str) + 3);
        let top = format!("╭─{}{}╮", title_str, "─".repeat(fill));
        push_grad_text(&mut out, &top, pal.grad, true);

        // Search input row
        let _ = write!(out, "\x1b[{};{}H", y0 + 2, x0 + 1);
        push_bg(&mut out, pal.tab_active_bg);
        push_fg(&mut out, pal.border, false);
        out.push('│');
        push_fg(&mut out, pal.grad[0], true);
        out.push_str(" ❯ ");
        push_fg(&mut out, pal.fg, false);
        let inner_w = modal_w.saturating_sub(2);
        let q_disp = truncate_to_cols(&pk.query, inner_w.saturating_sub(4));
        out.push_str(&q_disp);
        let q_len = 3 + str_display_width(&q_disp);
        for _ in q_len..inner_w {
            out.push(' ');
        }
        push_fg(&mut out, pal.border, false);
        out.push('│');
        prompt_cursor_pos = Some((y0 + 1, x0 + 1 + q_len.min(inner_w.saturating_sub(1))));

        // Divider
        let _ = write!(out, "\x1b[{};{}H", y0 + 3, x0 + 1);
        push_bg(&mut out, pal.bar_bg);
        push_fg(&mut out, pal.border, false);
        out.push('├');
        for _ in 0..inner_w {
            out.push('─');
        }
        out.push('┤');

        // Results
        let scroll_off = if pk.selected >= max_items {
            pk.selected + 1 - max_items
        } else {
            0
        };
        for row_i in 0..max_items {
            let _ = write!(out, "\x1b[{};{}H", y0 + 4 + row_i, x0 + 1);
            let idx_in_filtered = scroll_off + row_i;
            let is_sel = idx_in_filtered == pk.selected && idx_in_filtered < pk.filtered.len();
            push_bg(&mut out, if is_sel { pal.sel_bg } else { pal.bar_bg });
            push_fg(&mut out, pal.border, false);
            out.push('│');

            if let Some(scored) = pk.filtered.get(idx_in_filtered) {
                let item = &pk.items[scored.index];
                let detail_w = if item.detail.is_empty() {
                    0
                } else {
                    str_display_width(&item.detail) + 2
                };
                let avail_lbl = inner_w.saturating_sub(3 + detail_w);
                push_fg(&mut out, if is_sel { pal.grad[0] } else { pal.dim }, is_sel);
                out.push_str(if is_sel { " › " } else { "   " });

                let is_dir = matches!(item.action, PickerAction::BrowseDir(_));
                let mut written = 3usize;
                for (ci, ch) in item.label.chars().enumerate() {
                    let cw = char_display_width(ch);
                    if written + cw > 3 + avail_lbl {
                        break;
                    }
                    if scored.matched_chars.contains(&ci) {
                        push_fg(&mut out, pal.grad[0], true);
                    } else if is_dir {
                        push_fg(&mut out, pal.std_bi, is_sel);
                    } else {
                        push_fg(&mut out, pal.fg, is_sel);
                    }
                    out.push(ch);
                    written += cw;
                }
                let target_before_detail = inner_w.saturating_sub(detail_w);
                while written < target_before_detail {
                    out.push(' ');
                    written += 1;
                }
                if !item.detail.is_empty() && written + str_display_width(&item.detail) + 1 <= inner_w {
                    push_fg(&mut out, pal.dim, false);
                    out.push_str(&item.detail);
                    out.push(' ');
                    written += str_display_width(&item.detail) + 1;
                }
                while written < inner_w {
                    out.push(' ');
                    written += 1;
                }
            } else if row_i == 0 && pk.filtered.is_empty() && pk.create_base.is_some() && !pk.query.trim().is_empty() {
                let msg = format!(" + Criar e abrir '{}'", pk.query.trim());
                let disp = truncate_to_cols(&msg, inner_w);
                push_fg(&mut out, pal.ok, true);
                out.push_str(&disp);
                let written = str_display_width(&disp);
                for _ in written..inner_w {
                    out.push(' ');
                }
            } else {
                for _ in 0..inner_w {
                    out.push(' ');
                }
            }
            push_fg(&mut out, pal.border, false);
            out.push('│');
        }

        // Bottom border
        let _ = write!(out, "\x1b[{};{}H", y0 + 4 + max_items, x0 + 1);
        push_bg(&mut out, pal.bar_bg);
        let raw_hint = if pk.browse_dir.is_some() {
            format!(" {}/{} · Alt+↑/← subir · Enter abrir · Esc fechar ", pk.filtered.len(), pk.items.len())
        } else {
            format!(" {}/{} · Enter abrir · Esc fechar ", pk.filtered.len(), pk.items.len())
        };
        let hint = truncate_to_cols(&raw_hint, modal_w.saturating_sub(4));
        let bf = modal_w.saturating_sub(str_display_width(&hint) + 2);
        let bot = format!("╰{}{}─╯", "─".repeat(bf.saturating_sub(1)), hint);
        push_grad_text(&mut out, &bot, pal.grad, false);
        out.push_str(RST);
    }

    // -------------------------------------------------------------------------
    // Compact Floating Autocomplete Popup (anchored below or above cursor)
    // -------------------------------------------------------------------------
    if picker.is_none() && !has_prompt {
        if let (Some(comp), Some((cy, cx))) = (completion, primary_screen_pos) {
            if !comp.items.is_empty() && rows >= 8 && cols >= 30 {
                let max_vis = comp.items.len().min(6);
                let pop_h = max_vis + 2;
                let max_lbl = comp
                    .items
                    .iter()
                    .map(|it| str_display_width(&it.label) + str_display_width(&it.detail) + 10)
                    .max()
                    .unwrap_or(28);
                let pop_w = max_lbl.clamp(28, 48).min(cols.saturating_sub(2));
                let px = cx.min(cols.saturating_sub(pop_w + 1));
                let bottom_lim = term_panel
                    .map(|t| t.header_row)
                    .unwrap_or_else(|| rows.saturating_sub(1));
                let py = if cy + 1 + pop_h <= bottom_lim {
                    cy + 1
                } else {
                    cy.saturating_sub(pop_h)
                };
                let inner_w = pop_w.saturating_sub(2);
                let scroll_off = if comp.selected >= max_vis {
                    comp.selected + 1 - max_vis
                } else {
                    0
                };

                // Top border
                let _ = write!(out, "\x1b[{};{}H", py + 1, px + 1);
                push_bg(&mut out, pal.bar_bg);
                push_fg(&mut out, pal.border, false);
                out.push('╭');
                for _ in 0..inner_w {
                    out.push('─');
                }
                out.push('╮');

                // Candidate rows
                for vi in 0..max_vis {
                    let idx = scroll_off + vi;
                    let item = &comp.items[idx];
                    let is_sel = idx == comp.selected;
                    let _ = write!(out, "\x1b[{};{}H", py + 2 + vi, px + 1);
                    push_bg(&mut out, if is_sel { pal.sel_bg } else { pal.bar_bg });
                    push_fg(&mut out, pal.border, false);
                    out.push('│');

                    let tag_col = match item.kind {
                        CompletionKind::DeclaredVar => pal.ok,
                        CompletionKind::Identifier => pal.fg,
                        CompletionKind::Stl => pal.std_bi,
                        CompletionKind::Snippet => pal.grad[2],
                    };
                    let tag = format!(" {:<4} ", item.kind.tag());
                    push_fg(&mut out, tag_col, true);
                    out.push_str(&tag);
                    let mut w = str_display_width(&tag);

                    let det_w = if item.detail.is_empty() {
                        0
                    } else {
                        str_display_width(&item.detail) + 2
                    };
                    let lbl_max = inner_w.saturating_sub(w + det_w);
                    let lbl = truncate_to_cols(&item.label, lbl_max);
                    push_fg(&mut out, if is_sel { pal.grad[0] } else { pal.fg }, is_sel);
                    out.push_str(&lbl);
                    w += str_display_width(&lbl);

                    let before_det = inner_w.saturating_sub(det_w);
                    while w < before_det {
                        out.push(' ');
                        w += 1;
                    }
                    if det_w > 0 && w + det_w <= inner_w {
                        push_fg(&mut out, pal.dim, false);
                        out.push(' ');
                        out.push_str(&item.detail);
                        out.push(' ');
                        w += det_w;
                    }
                    while w < inner_w {
                        out.push(' ');
                        w += 1;
                    }
                    push_fg(&mut out, pal.border, false);
                    out.push('│');
                }

                // Bottom border with concise Tab hint
                let _ = write!(out, "\x1b[{};{}H", py + 2 + max_vis, px + 1);
                push_bg(&mut out, pal.bar_bg);
                let hint = truncate_to_cols(" Tab aceitar · Esc ", inner_w.saturating_sub(1));
                let hw = str_display_width(&hint);
                let fill = inner_w.saturating_sub(hw);
                push_fg(&mut out, pal.border, false);
                out.push('╰');
                for _ in 0..fill {
                    out.push('─');
                }
                push_fg(&mut out, pal.dim, false);
                out.push_str(&hint);
                push_fg(&mut out, pal.border, false);
                out.push('╯');
                out.push_str(RST);
            }
        }
    }

    // Position real hardware cursor (briefly hidden while the shooting-star core is physically flying!)
    if let Some((cy, cx)) = prompt_cursor_pos.or(term_cursor_pos) {
        let _ = write!(out, "\x1b[{};{}H\x1b[?25h", cy + 1, cx + 1);
    } else if !hide_editor_cursor {
        if let Some((cy, cx)) = primary_screen_pos {
            let _ = write!(out, "\x1b[{};{}H\x1b[?25h", cy + 1, cx + 1);
        }
    }

    if synced {
        out.push_str(SYNC_END);
    }
    (out, layout)
}

fn render_pane(
    out: &mut String,
    doc: &mut Document,
    p: &PaneRenderInput,
    multi_pane: bool,
    sb_hot: bool,
    diagnostics: &[Diagnostic],
    ghost_suffix: Option<&str>,
    trail_cells: &HashMap<(usize, usize), TrailCell>,
    pal: &Palette,
    cfg: &EditorConfig,
) -> (PaneLayoutInfo, Option<(usize, usize)>, usize, usize, usize, usize) {
    let rect = p.rect;
    let wrap = cfg.wrap;
    let total_lines = doc.rope.line_count();
    let content_h = rect.h.saturating_sub(1);
    let content_start_row = rect.y + 1;

    let raw_gutter_w = compute_gutter_width(total_lines);
    let show_gutter = rect.w >= raw_gutter_w + 4;
    let gutter_w = if show_gutter { raw_gutter_w } else { 0 };
    let num_digits = gutter_w.saturating_sub(5);
    let show_sb = cfg.scrollbar && rect.w >= gutter_w + 16;
    let sb_w = if show_sb { 1 } else { 0 };
    let text_w = rect.w.saturating_sub(gutter_w + sb_w);

    let mut p_layout = PaneLayoutInfo {
        pane_id: p.pane_id,
        rect,
        gutter_width: gutter_w,
        content_start_row,
        content_height: content_h,
        scrollbar_col: if show_sb && rect.w > 0 {
            Some(rect.x + rect.w - 1)
        } else {
            None
        },
        visual_rows: Vec::with_capacity(content_h),
        tab_bar_row: rect.y,
        tab_hitboxes: Vec::new(),
    };

    // -------------------------------------------------------------------------
    // Row `rect.y`: Pane Header / Tab Bar (exact `rect.w` columns)
    // -------------------------------------------------------------------------
    let _ = write!(out, "\x1b[{};{}H", rect.y + 1, rect.x + 1);
    let bar_base_bg = if multi_pane && p.is_focused {
        pal.tab_active_bg
    } else {
        pal.bar_bg
    };
    push_bg(out, bar_base_bg);
    let mut used_cols = 0usize;

    if !multi_pane {
        // Single-pane mode: exact signature NAVE EDIT brand + tabs + right metadata
        let brand = " NAVE EDIT ";
        let bw = brand.chars().count();
        if bw + 2 <= rect.w {
            push_grad_text(out, brand, pal.grad, true);
            used_cols += bw;
            push_fg(out, pal.border, false);
            out.push('│');
            used_cols += 1;
        }
    } else {
        // Multi-pane mode: compact active/inactive group badge ` ① `
        let badge = format!(" {} ", group_badge_glyph(p.group_num));
        let bw = str_display_width(&badge);
        if bw <= rect.w {
            if p.is_focused {
                push_fg(out, pal.grad[0], true);
            } else {
                push_fg(out, pal.dim, false);
            }
            out.push_str(&badge);
            used_cols += bw;
        }
    }

    let right_meta = if !multi_pane {
        format!(
            " [{} · {}{}{}] [{}] ",
            doc.lang.name(),
            doc.eol.label(),
            if wrap { " · WRAP" } else { "" },
            if cfg.relative_numbers { " · REL" } else { "" },
            pal.name
        )
    } else {
        String::new()
    };
    let right_w = str_display_width(&right_meta);
    let max_tabs_col = if !multi_pane && right_w + 2 < rect.w {
        rect.w - right_w - 1
    } else {
        rect.w
    };

    // Ensure the active tab is visible even in narrow panes
    let active_idx = p.tabs.iter().position(|t| t.active).unwrap_or(0);
    let mut start_tab = 0usize;
    if p.tabs.len() > 1 {
        let mut w_to_active = 0usize;
        for t in p.tabs.iter().take(active_idx + 1) {
            let lbl_w = str_display_width(&t.name) + if t.dirty { 4 } else { 2 } + 1;
            w_to_active += lbl_w;
        }
        if used_cols + w_to_active > max_tabs_col {
            start_tab = active_idx;
        }
    }

    for (idx, t) in p.tabs.iter().enumerate().skip(start_tab) {
        let full_label = if t.dirty {
            format!(" {} ● ", t.name)
        } else {
            format!(" {} ", t.name)
        };
        let avail = max_tabs_col.saturating_sub(used_cols);
        if avail < 3 {
            break;
        }
        let label = if str_display_width(&full_label) > avail {
            if idx == active_idx || idx == start_tab {
                truncate_to_cols(&full_label, avail)
            } else {
                break;
            }
        } else {
            full_label
        };
        let lw = str_display_width(&label);
        let abs_start = rect.x + used_cols;
        if t.active {
            if p.is_focused {
                push_bg(out, pal.tab_active_bg);
                push_fg(out, pal.grad[0], true);
            } else {
                push_bg(out, pal.bar_bg);
                push_fg(out, pal.fg, false);
            }
            out.push_str(&label);
            push_bg(out, bar_base_bg);
        } else {
            push_fg(out, pal.dim, false);
            out.push_str(&label);
        }
        p_layout.tab_hitboxes.push((abs_start, abs_start + lw, idx));
        used_cols += lw;
        if used_cols < max_tabs_col {
            push_fg(out, pal.border, false);
            out.push('│');
            used_cols += 1;
        }
    }

    if !right_meta.is_empty() && used_cols + right_w <= rect.w {
        let pad = rect.w - used_cols - right_w;
        for _ in 0..pad {
            out.push(' ');
        }
        push_fg(out, pal.dim, false);
        out.push_str(&right_meta);
    } else {
        while used_cols < rect.w {
            out.push(' ');
            used_cols += 1;
        }
    }
    out.push_str(RST);

    // -------------------------------------------------------------------------
    // Pane Editor Canvas (`rect.y + 1 .. rect.y + rect.h`)
    // -------------------------------------------------------------------------
    let cursors = if p.cursors.is_empty() {
        vec![Selection::caret(0)]
    } else {
        p.cursors.clone()
    };
    let primary_idx = p.primary.min(cursors.len() - 1);
    let primary_sel = cursors[primary_idx];
    let primary_head = doc.clamp_byte_pos(primary_sel.head);
    let (primary_row, primary_col) = doc.rope.byte_to_line_col(primary_head);

    let bracket_pair = if p.is_focused && cfg.bracket_match {
        find_matching_bracket(&doc.rope, primary_head, cfg.block_cursor)
    } else {
        None
    };
    let git_marks: HashMap<usize, GutterMark> = doc.git.snapshot();
    let has_custom_hash = if cfg.cp_radar && doc.lang == crate::syntax::Language::CLike {
        file_has_custom_hash(&doc.rope)
    } else {
        true
    };

    // Right-edge scrollbar geometry
    let (sb_start, sb_end, sb_cursor_row) = if show_sb && content_h > 0 {
        let tot = total_lines.max(1);
        let thumb_h = ((content_h * content_h) / tot).clamp(1, content_h);
        let max_scroll = tot.saturating_sub(content_h).max(1);
        let start = if tot <= content_h {
            0
        } else {
            (p.scroll_row.min(max_scroll) * (content_h - thumb_h)) / max_scroll
        };
        let cur_y = (primary_row * content_h.saturating_sub(1)) / tot.saturating_sub(1).max(1);
        (start, (start + thumb_h).min(content_h), cur_y.min(content_h.saturating_sub(1)))
    } else {
        (0, 0, 0)
    };

    let sel_ranges: Vec<(usize, usize)> = cursors
        .iter()
        .filter(|c| !c.is_empty())
        .map(|c| {
            let (s, e) = c.range();
            (doc.clamp_byte_pos(s), doc.clamp_byte_pos(e))
        })
        .collect();
    let sel_chars: usize = sel_ranges
        .iter()
        .map(|&(s, e)| doc.rope.slice(s, e).chars().count())
        .sum();
    let secondary_carets: Vec<usize> = if p.is_focused {
        cursors
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != primary_idx)
            .map(|(_, c)| doc.clamp_byte_pos(c.head))
            .collect()
    } else {
        Vec::new()
    };

    // Cursorword (only in focused pane)
    let cursorword_ranges: Vec<(usize, usize)> =
        if p.is_focused && cfg.cursorword && doc.search.query.is_empty() {
            if let Some(word) = doc.word_at_primary() {
                let mut found = Vec::new();
                let last_vis = (p.scroll_row + content_h + 2).min(total_lines);
                for r in p.scroll_row..last_vis {
                    let ls = doc.rope.line_start_byte(r);
                    let txt = doc.rope.line(r);
                    for (s, e) in find_whole_word_in_line(&txt, &word) {
                        found.push((ls + s, ls + e));
                    }
                }
                if found.len() >= 2 { found } else { Vec::new() }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

    let indent_w = doc.tab_width.max(1);
    let active_scope: Option<(usize, usize, usize)> = if p.is_focused && cfg.indent_guides {
        compute_active_indent_scope(&doc.rope, primary_row, indent_w)
    } else {
        None
    };

    let mut primary_screen_pos: Option<(usize, usize)> = None;
    let mut screen_y = 0usize;
    let mut doc_row = p.scroll_row.min(total_lines.saturating_sub(1));

    while screen_y < content_h {
        let abs_row = content_start_row + screen_y;

        if doc_row >= total_lines {
            let _ = write!(out, "\x1b[{};{}H", abs_row + 1, rect.x + 1);
            push_base_bg(out, cfg, pal);
            if show_gutter {
                let g_str = format!("  {:>width$} │ ", "~", width = num_digits);
                if trail_cells.is_empty() {
                    push_fg(out, pal.border, false);
                    out.push_str(&g_str);
                } else {
                    for (gx, ch) in g_str.chars().enumerate() {
                        if let Some(tc) = trail_cells.get(&(abs_row, rect.x + gx)) {
                            push_bg(out, tc.bg);
                            push_fg(out, if tc.is_hot { pal.bracket_fg } else { pal.fg }, tc.is_hot);
                        } else {
                            push_base_bg(out, cfg, pal);
                            push_fg(out, pal.border, false);
                        }
                        out.push(ch);
                    }
                }
            }
            out.push_str(RST);
            if trail_cells.is_empty() {
                push_base_bg(out, cfg, pal);
                for _ in 0..text_w {
                    out.push(' ');
                }
            } else {
                for tx in 0..text_w {
                    if let Some(tc) = trail_cells.get(&(abs_row, rect.x + gutter_w + tx)) {
                        push_bg(out, tc.bg);
                        if let Some((dch, dfg)) = tc.dust {
                            push_fg(out, dfg, false);
                            out.push(dch);
                        } else {
                            out.push(' ');
                        }
                    } else {
                        push_base_bg(out, cfg, pal);
                        out.push(' ');
                    }
                }
            }
            if show_sb {
                if let Some(tc) = trail_cells.get(&(abs_row, rect.x + gutter_w + text_w)) {
                    push_bg(out, tc.bg);
                } else {
                    push_base_bg(out, cfg, pal);
                }
                push_scrollbar_cell(out, screen_y, content_h, sb_start, sb_end, sb_cursor_row, sb_hot, pal);
            }
            out.push_str(RST);
            screen_y += 1;
            continue;
        }

        let line_start = doc.rope.line_start_byte(doc_row);
        let line_text = doc.rope.line(doc_row);
        let spans = doc.syntax.line_spans(&doc.rope, doc_row);
        let is_cur_line = p.is_focused && doc_row == primary_row;

        let char_info = build_char_cells(&line_text, line_start, &spans, doc.tab_width);
        let segments: Vec<(usize, usize)> = if wrap && text_w > 0 {
            split_wrapped_segments(&char_info, text_w)
        } else {
            vec![(0, char_info.len())]
        };

        for (seg_idx, (seg_start, seg_end)) in segments.into_iter().enumerate() {
            if screen_y >= content_h {
                break;
            }
            let abs_row = content_start_row + screen_y;
            let _ = write!(out, "\x1b[{};{}H", abs_row + 1, rect.x + 1);

            p_layout.visual_rows.push(VisualRowInfo {
                doc_row,
                start_char_col: seg_start,
                end_char_col: seg_end,
            });

            // Gutter
            if show_gutter {
                let is_bookmarked = doc.bookmarks.contains(&doc_row);
                let mut g_col = 0usize;
                let emit_g = |out: &mut String, g_col: &mut usize, s: &str, fg: Rgb, bold: bool| {
                    if trail_cells.is_empty() {
                        if is_cur_line {
                            push_bg(out, pal.cur_line_bg);
                        } else {
                            push_base_bg(out, cfg, pal);
                        }
                        push_fg(out, fg, bold);
                        out.push_str(s);
                        *g_col += s.chars().count();
                    } else {
                        for ch in s.chars() {
                            if let Some(tc) = trail_cells.get(&(abs_row, rect.x + *g_col)) {
                                push_bg(out, tc.bg);
                                push_fg(out, if tc.is_hot { pal.bracket_fg } else { fg }, bold || tc.is_hot);
                            } else {
                                if is_cur_line {
                                    push_bg(out, pal.cur_line_bg);
                                } else {
                                    push_base_bg(out, cfg, pal);
                                }
                                push_fg(out, fg, bold);
                            }
                            out.push(ch);
                            *g_col += 1;
                        }
                    }
                };

                if seg_idx == 0 {
                    let line_diag = diagnostics.iter().find(|d| d.line == doc_row);
                    let has_radar = cfg.cp_radar
                        && line_diag.is_none()
                        && detect_cp_pitfall(&line_text, doc.lang, has_custom_hash).is_some();
                    if let Some(d) = line_diag {
                        match d.severity {
                            DiagSeverity::Error => emit_g(out, &mut g_col, "●", pal.err, true),
                            DiagSeverity::Warning => emit_g(out, &mut g_col, "▲", pal.warn, true),
                        }
                    } else if has_radar {
                        emit_g(out, &mut g_col, "!", pal.warn, true);
                    } else if is_bookmarked {
                        emit_g(out, &mut g_col, "◆", pal.grad[2], true);
                    } else if is_cur_line {
                        emit_g(out, &mut g_col, "▸", pal.active_line_num, true);
                    } else {
                        emit_g(out, &mut g_col, " ", pal.line_num, false);
                    }
                    if is_cur_line {
                        let s = format!(" {:>width$} ", doc_row + 1, width = num_digits);
                        emit_g(out, &mut g_col, &s, pal.active_line_num, true);
                    } else if cfg.relative_numbers {
                        let dist = doc_row.abs_diff(primary_row);
                        let s = format!(" {:>width$} ", dist, width = num_digits);
                        emit_g(out, &mut g_col, &s, pal.line_num, false);
                    } else {
                        let s = format!(" {:>width$} ", doc_row + 1, width = num_digits);
                        emit_g(out, &mut g_col, &s, pal.line_num, false);
                    }
                } else {
                    let s = format!("  {:>width$} ", "↪", width = num_digits);
                    emit_g(out, &mut g_col, &s, pal.dim, false);
                }

                match git_marks.get(&doc_row) {
                    Some(GutterMark::Added) if seg_idx == 0 => emit_g(out, &mut g_col, "▎ ", pal.ok, false),
                    Some(GutterMark::Modified) if seg_idx == 0 => emit_g(out, &mut g_col, "▎ ", pal.warn, false),
                    Some(GutterMark::Deleted) if seg_idx == 0 => emit_g(out, &mut g_col, "▁ ", pal.err, false),
                    _ => emit_g(out, &mut g_col, "│ ", pal.border, false),
                }
            }

            // Content cells
            let h_off = if wrap { 0 } else { p.scroll_col };
            let mut cur_vis = 0usize;
            let mut drawn_cols = 0usize;

            if is_cur_line {
                let on_this_seg = if !wrap {
                    true
                } else {
                    (primary_col >= seg_start && primary_col < seg_end)
                        || (seg_end == char_info.len() && primary_col == seg_end)
                };
                if on_this_seg {
                    let mut c_vis = 0usize;
                    for cell in &char_info[seg_start..primary_col.min(seg_end)] {
                        c_vis += cell.width;
                    }
                    if c_vis >= h_off && c_vis - h_off < text_w {
                        primary_screen_pos = Some((abs_row, rect.x + gutter_w + (c_vis - h_off)));
                    }
                }
            }

            let is_blank_line = line_text.trim().is_empty();
            let lead_vis = if cfg.indent_guides {
                if is_blank_line {
                    blank_line_inherited_indent(&doc.rope, doc_row, indent_w)
                } else {
                    leading_visual_width(&line_text, indent_w)
                }
            } else {
                0
            };
            let trail_start_idx = if cfg.show_trailing && !is_blank_line {
                let trimmed_chars = line_text.trim_end_matches(|c| c == ' ' || c == '\t').chars().count();
                if is_cur_line && primary_col >= trimmed_chars {
                    char_info.len()
                } else {
                    trimmed_chars
                }
            } else {
                char_info.len()
            };
            let ruler_col = if !wrap && cfg.color_column > 0 {
                Some(cfg.color_column)
            } else {
                None
            };

            for (local_i, cell) in char_info[seg_start..seg_end].iter().enumerate() {
                let char_idx = seg_start + local_i;
                let cw = cell.width;
                if cur_vis + cw <= h_off {
                    cur_vis += cw;
                    continue;
                }
                if drawn_cols + cw > text_w {
                    break;
                }

                let byte_abs = cell.abs_byte;
                let is_sec_caret = secondary_carets.contains(&byte_abs);
                let is_selected = sel_ranges.iter().any(|&(s, e)| byte_abs >= s && byte_abs < e);
                let is_active_match = p.is_focused
                    && doc
                        .search
                        .active_match()
                        .is_some_and(|(s, e)| byte_abs >= s && byte_abs < e);
                let is_match = !is_active_match
                    && doc
                        .search
                        .matches
                        .iter()
                        .any(|&(s, e)| byte_abs >= s && byte_abs < e);
                let is_bracket = bracket_pair.is_some_and(|(a, b)| byte_abs == a || byte_abs == b);
                let is_cursorword = !is_selected
                    && !is_match
                    && !is_active_match
                    && cursorword_ranges
                        .iter()
                        .any(|&(s, e)| byte_abs >= s && byte_abs < e);
                let is_ruler = ruler_col == Some(cur_vis);

                let eff_kind = match cell.kind {
                    TokenKind::Rainbow(_) | TokenKind::BracketError if !cfg.rainbow_brackets => {
                        TokenKind::Punctuation
                    }
                    TokenKind::TodoBadge | TokenKind::FixmeBadge if !cfg.comment_badges => {
                        TokenKind::Comment
                    }
                    other => other,
                };
                let style = pal.token_style(eff_kind);
                let mut fg_c = style.fg;
                let mut bold = style.bold;
                let mut italic = style.italic;
                let mut underline = false;
                let mut glyph_override: Option<char> = None;

                let mut bg_c = if is_sec_caret {
                    fg_c = pal.bar_bg;
                    bold = true;
                    italic = false;
                    Some(pal.grad[0])
                } else if is_selected {
                    Some(pal.sel_bg)
                } else if is_active_match {
                    bold = true;
                    Some(pal.active_match_bg)
                } else if is_match {
                    Some(pal.match_bg)
                } else if is_bracket {
                    fg_c = pal.bracket_fg;
                    bold = true;
                    italic = false;
                    underline = true;
                    Some(pal.bracket_bg)
                } else if is_cursorword {
                    underline = true;
                    Some(pal.word_hl_bg)
                } else if let Some(tok_bg) = style.bg {
                    Some(tok_bg)
                } else if is_cur_line {
                    Some(pal.cur_line_bg)
                } else if is_ruler {
                    Some(pal.cur_line_bg)
                } else {
                    None
                };

                if style.bg.is_some() && !is_sec_caret && !is_bracket && bg_c != style.bg {
                    fg_c = pal.fg;
                }

                // Luminous cursor travel trail overlay (preserves code text razor-sharp!)
                if !trail_cells.is_empty() && !is_sec_caret {
                    if let Some(tc) = trail_cells.get(&(abs_row, rect.x + gutter_w + drawn_cols)) {
                        bg_c = Some(tc.bg);
                        if tc.is_hot {
                            fg_c = pal.bracket_fg;
                            bold = true;
                            italic = false;
                        }
                    }
                }

                if !is_sec_caret && !is_selected && !is_match && !is_active_match {
                    if char_idx >= trail_start_idx && (cell.ch == ' ' || cell.ch == '\t') {
                        fg_c = pal.err;
                        italic = false;
                        glyph_override = Some('·');
                    } else if cfg.indent_guides
                        && cell.ch == ' '
                        && cur_vis > 0
                        && cur_vis < lead_vis
                        && cur_vis % indent_w == 0
                    {
                        let in_active_scope = active_scope.is_some_and(|(s_r, e_r, a_col)| {
                            doc_row >= s_r && doc_row <= e_r && cur_vis == a_col
                        });
                        fg_c = if in_active_scope { pal.indent_active } else { pal.indent_guide };
                        italic = false;
                        glyph_override = Some('▏');
                    }
                }

                if let Some(bg) = bg_c {
                    push_bg(out, bg);
                } else {
                    push_base_bg(out, cfg, pal);
                }
                push_fg_styled(out, fg_c, bold, italic);
                if underline {
                    out.push_str("\x1b[4m");
                }

                if cell.ch == '\t' {
                    if let Some(g) = glyph_override {
                        out.push(g);
                        for _ in 1..cw {
                            out.push(' ');
                        }
                    } else {
                        for _ in 0..cw {
                            out.push(' ');
                        }
                    }
                } else {
                    out.push(glyph_override.unwrap_or(cell.ch));
                }
                if underline {
                    out.push_str("\x1b[24m");
                }
                cur_vis += cw;
                drawn_cols += cw;
            }

            // Secondary caret at end-of-line
            let eol_byte = line_start + line_text.len();
            if seg_end == char_info.len()
                && secondary_carets.contains(&eol_byte)
                && drawn_cols < text_w
                && cur_vis >= h_off
            {
                push_bg(out, pal.grad[0]);
                push_fg(out, pal.bar_bg, true);
                out.push(' ');
                drawn_cols += 1;
            }

            // Inline ghost autocomplete suffix right after primary caret at EOL
            if is_cur_line && seg_end == char_info.len() && primary_col == char_info.len() {
                if let Some(ghost) = ghost_suffix {
                    push_bg(out, pal.cur_line_bg);
                    push_fg_styled(out, pal.dim, false, true);
                    for gch in ghost.chars() {
                        let gw = char_display_width(gch);
                        if drawn_cols + gw > text_w {
                            break;
                        }
                        out.push(gch);
                        drawn_cols += gw;
                    }
                }
            }

            // Fill remaining width to EXACT `text_w` columns (never `\x1b[K`!)
            while drawn_cols < text_w {
                let vis_x = h_off + drawn_cols;
                let is_blank_guide = is_blank_line
                    && cfg.indent_guides
                    && seg_idx == 0
                    && vis_x > 0
                    && vis_x < lead_vis
                    && vis_x % indent_w == 0;
                let is_ruler = ruler_col == Some(vis_x);

                let trail_hit = if trail_cells.is_empty() {
                    None
                } else {
                    trail_cells.get(&(abs_row, rect.x + gutter_w + drawn_cols))
                };
                if let Some(tc) = trail_hit {
                    push_bg(out, tc.bg);
                } else if is_cur_line || is_ruler {
                    push_bg(out, pal.cur_line_bg);
                } else {
                    push_base_bg(out, cfg, pal);
                }
                if is_blank_guide {
                    let in_active_scope = active_scope.is_some_and(|(s_r, e_r, a_col)| {
                        doc_row >= s_r && doc_row <= e_r && vis_x == a_col
                    });
                    push_fg(out, if in_active_scope { pal.indent_active } else { pal.indent_guide }, false);
                    out.push('▏');
                } else if let Some((dch, dfg)) = trail_hit.and_then(|tc| tc.dust) {
                    push_fg(out, dfg, false);
                    out.push(dch);
                } else {
                    out.push(' ');
                }
                drawn_cols += 1;
            }

            if show_sb {
                if let Some(tc) = trail_cells.get(&(abs_row, rect.x + gutter_w + text_w)) {
                    push_bg(out, tc.bg);
                } else {
                    push_base_bg(out, cfg, pal);
                }
                push_scrollbar_cell(out, screen_y, content_h, sb_start, sb_end, sb_cursor_row, sb_hot, pal);
            }
            out.push_str(RST);
            screen_y += 1;
        }

        doc_row += 1;
    }

    (p_layout, primary_screen_pos, sel_chars, primary_row, primary_col, cursors.len())
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn find_whole_word_in_line(line: &str, word: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let wlen = word.len();
    if wlen == 0 || line.len() < wlen {
        return out;
    }
    let bytes = line.as_bytes();
    let mut off = 0;
    while let Some(rel) = line[off..].find(word) {
        let s = off + rel;
        let e = s + wlen;
        let left_ok = s == 0 || !is_ident_byte(bytes[s - 1]);
        let right_ok = e == bytes.len() || !is_ident_byte(bytes[e]);
        if left_ok && right_ok {
            out.push((s, e));
        }
        off = e;
    }
    out
}

fn leading_visual_width(line: &str, tab_w: usize) -> usize {
    let mut v = 0usize;
    for ch in line.chars() {
        if ch == ' ' {
            v += 1;
        } else if ch == '\t' {
            v += tab_w - (v % tab_w);
        } else {
            break;
        }
    }
    v
}

fn blank_line_inherited_indent(rope: &crate::rope::Rope, row: usize, tab_w: usize) -> usize {
    let total = rope.line_count();
    let mut above = 0usize;
    for r in (row.saturating_sub(8)..row).rev() {
        let t = rope.line(r);
        if !t.trim().is_empty() {
            above = leading_visual_width(&t, tab_w);
            break;
        }
    }
    let mut below = 0usize;
    for r in (row + 1)..(row + 9).min(total) {
        let t = rope.line(r);
        if !t.trim().is_empty() {
            below = leading_visual_width(&t, tab_w);
            break;
        }
    }
    above.min(below)
}

fn compute_active_indent_scope(
    rope: &crate::rope::Rope,
    primary_row: usize,
    indent_w: usize,
) -> Option<(usize, usize, usize)> {
    let total = rope.line_count();
    let cur_txt = rope.line(primary_row);
    let cur_lead = if cur_txt.trim().is_empty() {
        blank_line_inherited_indent(rope, primary_row, indent_w)
    } else {
        leading_visual_width(&cur_txt, indent_w)
    };
    let active_col = (cur_lead / indent_w) * indent_w;
    if active_col == 0 {
        return None;
    }
    let mut s = primary_row;
    while s > 0 && primary_row - s < 80 {
        let t = rope.line(s - 1);
        if t.trim().is_empty() || leading_visual_width(&t, indent_w) >= active_col {
            s -= 1;
        } else {
            break;
        }
    }
    let mut e = primary_row;
    while e + 1 < total && e - primary_row < 80 {
        let t = rope.line(e + 1);
        if t.trim().is_empty() || leading_visual_width(&t, indent_w) >= active_col {
            e += 1;
        } else {
            break;
        }
    }
    Some((s, e, active_col))
}

fn push_scrollbar_cell(
    out: &mut String,
    screen_y: usize,
    content_h: usize,
    sb_start: usize,
    sb_end: usize,
    sb_cursor_row: usize,
    sb_hot: bool,
    pal: &Palette,
) {
    let t = screen_y as f32 / content_h.saturating_sub(1).max(1) as f32;
    if screen_y == sb_cursor_row {
        push_fg(out, pal.grad[0], true);
        out.push('█');
    } else if screen_y >= sb_start && screen_y < sb_end {
        if sb_hot {
            push_fg(out, pal.grad[0], true);
            out.push('█');
        } else {
            push_fg(out, lerp_grad(pal.grad, t), false);
            out.push('┃');
        }
    } else {
        push_fg(out, if sb_hot { pal.dim } else { pal.border }, false);
        out.push('│');
    }
}

#[derive(Clone, Copy)]
struct CharCell {
    ch: char,
    abs_byte: usize,
    width: usize,
    kind: TokenKind,
}

fn build_char_cells(line: &str, line_start: usize, spans: &[Span], tab_w: usize) -> Vec<CharCell> {
    let mut cells = Vec::with_capacity(line.len());
    let mut span_idx = 0usize;
    let mut vis = 0usize;

    for (rel_b, ch) in line.char_indices() {
        while span_idx < spans.len() && spans[span_idx].end <= rel_b {
            span_idx += 1;
        }
        let kind = if span_idx < spans.len() && rel_b >= spans[span_idx].start && rel_b < spans[span_idx].end {
            spans[span_idx].kind
        } else {
            TokenKind::Plain
        };
        let w = if ch == '\t' {
            tab_w - (vis % tab_w)
        } else {
            char_display_width(ch)
        };
        vis += w;
        cells.push(CharCell {
            ch,
            abs_byte: line_start + rel_b,
            width: w,
            kind,
        });
    }
    cells
}

fn split_wrapped_segments(cells: &[CharCell], max_w: usize) -> Vec<(usize, usize)> {
    if cells.is_empty() {
        return vec![(0, 0)];
    }
    let mut segs = Vec::new();
    let mut start = 0usize;
    let mut w = 0usize;
    for (i, c) in cells.iter().enumerate() {
        if w + c.width > max_w && i > start {
            segs.push((start, i));
            start = i;
            w = 0;
        }
        w += c.width;
    }
    segs.push((start, cells.len()));
    segs
}

fn str_display_width(s: &str) -> usize {
    s.chars().map(char_display_width).sum()
}

fn truncate_to_cols(s: &str, max_cols: usize) -> String {
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = char_display_width(ch);
        if w + cw > max_cols {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn cursor_trail_skips_1cell_steps_and_animates_jumps_smoothly() {
        let now = Instant::now();
        // Adjacent 1-column move should NOT spawn a trail
        assert!(CursorTrail::new((5, 10), (5, 11), now).is_none());

        // Word jump (Ctrl+Right) spawns a crisp horizontal path
        let tr = CursorTrail::new((5, 10), (5, 22), now).expect("word jump should spawn trail");
        assert_eq!(tr.path.first(), Some(&(5, 10)));
        assert_eq!(tr.path.last(), Some(&(5, 22)));
        assert!(tr.path.len() >= 13);

        // Cross-pane diagonal jump (Ctrl+2) spawns continuous path
        let diag = CursorTrail::new((12, 15), (4, 80), now).expect("pane jump should spawn trail");
        assert_eq!(diag.path.first(), Some(&(12, 15)));
        assert_eq!(diag.path.last(), Some(&(4, 80)));

        let pal = &crate::palette::PALETTES[0];
        // At t=0, C2 smootherstep starts at head_u = 0 (finite acceleration, zero teleport!)
        assert_eq!(tr.head_u(now), 0.0);
        let m0 = build_trail_map(&[tr.clone()], now, pal);
        assert!(m0.contains_key(&(5, 10)), "star should ignite right at start position on frame 0");
        assert!(!m0.contains_key(&(5, 22)), "star must not teleport to destination on frame 0");
        // Pure horizontal jump stays strictly on row 5 (1-line laser streak)
        let mid_h = build_trail_map(&[tr.clone()], now + Duration::from_millis(65), pal);
        assert!(!mid_h.is_empty());
        assert!(mid_h.keys().all(|&(r, _)| r == 5), "horizontal jump must stay 1-row thin");

        // Diagonal jump fans out into a 2D conical plume strictly wider than its 1-cell spine!
        let mid_d = build_trail_map(&[diag.clone()], now + Duration::from_millis(95), pal);
        let spine_set: std::collections::HashSet<(usize, usize)> = diag.path.iter().copied().collect();
        let off_spine = mid_d.keys().filter(|k| !spine_set.contains(k)).count();
        assert!(off_spine >= 6, "diagonal jump should render a 2D conical wake, off_spine={}", off_spine);

        let done_t = now + Duration::from_millis(260);
        assert!(tr.is_finished(done_t));
        assert!(build_trail_map(&[tr], done_t, pal).is_empty(), "trail must vanish completely after duration");
    }
}
