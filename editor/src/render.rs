use crate::config::EditorConfig;
use crate::document::{Document, Selection};
use crate::git::GutterMark;
use crate::layout::{ComputedLayout, DividerHit, PaneId, Rect, SplitAxis};
use crate::palette::{lerp_grad, push_bg, push_fg, push_fg_styled, push_grad_text, Palette, TokenKind};
use crate::picker::Picker;
use crate::syntax::{find_matching_bracket, Span};
use std::collections::HashMap;
use std::fmt::Write as _;

const SYNC_START: &str = "\x1b[?2026h";
const SYNC_END: &str = "\x1b[?2026l";
const RST: &str = "\x1b[0m";

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
    pub visual_rows: Vec<VisualRowInfo>,
    pub tab_bar_row: usize,
    pub tab_hitboxes: Vec<(usize, usize, usize)>, // (start_col, end_col, tab_idx)
}

#[derive(Clone, Debug, Default)]
pub struct LayoutInfo {
    pub panes: Vec<PaneLayoutInfo>,
    pub dividers: Vec<DividerHit>,
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
    highlight_all_dividers: bool,
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

        let (p_layout, p_cursor, sel_chars, cur_row, cur_col, cur_count) = render_pane(
            &mut out,
            doc,
            p,
            multi_pane,
            pal,
            cfg,
        );

        if p.is_focused {
            primary_screen_pos = p_cursor;
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
        out.push_str("\x1b[49m");
        for d in &computed.dividers {
            let is_hot = highlight_all_dividers || active_divider == Some(d.split_id);
            if is_hot {
                push_fg(&mut out, pal.grad[0], true);
            } else {
                push_fg(&mut out, pal.border, false);
            }
            match d.axis {
                SplitAxis::Horizontal => {
                    for r in d.y..(d.y + d.len) {
                        let _ = write!(out, "\x1b[{};{}H│", r + 1, d.x + 1);
                    }
                }
                SplitAxis::Vertical => {
                    let _ = write!(out, "\x1b[{};{}H", d.y + 1, d.x + 1);
                    for _ in 0..d.len {
                        out.push('─');
                    }
                }
            }
        }
        if !computed.junctions.is_empty() {
            if highlight_all_dividers {
                push_fg(&mut out, pal.grad[0], true);
            } else {
                push_fg(&mut out, pal.border, false);
            }
            for &(jx, jy, jch) in &computed.junctions {
                let _ = write!(out, "\x1b[{};{}H{}", jy + 1, jx + 1, jch);
            }
        }
        out.push_str(RST);
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
    let left_status = format!(
        " Ln {}, Col {}{}{}",
        primary_row + 1,
        primary_col + 1,
        multi_badge,
        pane_badge
    );
    push_fg(&mut out, pal.grad[0], true);
    let left_disp = truncate_to_cols(&left_status, cols);
    out.push_str(&left_disp);

    let mut used_s = str_display_width(&left_disp);
    if let Some(hint) = mode_hint {
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
        let m = "  │  ⚠ Alterado no disco (F5 recarregar)";
        push_fg(&mut out, pal.warn, true);
        let disp = truncate_to_cols(m, cols.saturating_sub(used_s));
        out.push_str(&disp);
        used_s += str_display_width(&disp);
    }

    let right_help = if multi_pane {
        format!(
            "[{} · {}{}{}] [{}] · Ctrl+K painel · F1 cmds ",
            lang_name,
            eol_lbl,
            if cfg.wrap { " · WRAP" } else { "" },
            if cfg.relative_numbers { " · REL" } else { "" },
            pal.name
        )
    } else {
        "Ctrl+S salvar · Ctrl+\\ dividir · Ctrl+K painel · Ctrl+P abrir · F1 cmds ".into()
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
        let title_str = format!(" {} ", pk.title);
        let fill = modal_w.saturating_sub(title_str.chars().count() + 3);
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

                let mut written = 3usize;
                for (ci, ch) in item.label.chars().enumerate() {
                    let cw = char_display_width(ch);
                    if written + cw > 3 + avail_lbl {
                        break;
                    }
                    if scored.matched_chars.contains(&ci) {
                        push_fg(&mut out, pal.grad[0], true);
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
        let hint = format!(" {}/{} · Enter abrir · Esc fechar ", pk.filtered.len(), pk.items.len());
        let bf = modal_w.saturating_sub(hint.chars().count() + 2);
        let bot = format!("╰{}{}─╯", "─".repeat(bf.saturating_sub(1)), hint);
        push_grad_text(&mut out, &bot, pal.grad, false);
        out.push_str(RST);
    }

    // Position real hardware cursor
    if let Some((cy, cx)) = prompt_cursor_pos.or(primary_screen_pos) {
        let _ = write!(out, "\x1b[{};{}H\x1b[?25h", cy + 1, cx + 1);
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
            out.push_str("\x1b[49m");
            if show_gutter {
                push_fg(out, pal.border, false);
                let _ = write!(out, "  {:>width$} │ ", "~", width = num_digits);
            }
            out.push_str(RST);
            for _ in 0..text_w {
                out.push(' ');
            }
            if show_sb {
                push_scrollbar_cell(out, screen_y, content_h, sb_start, sb_end, sb_cursor_row, pal);
                out.push_str(RST);
            }
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
                if is_cur_line {
                    push_bg(out, pal.cur_line_bg);
                } else {
                    out.push_str("\x1b[49m");
                }
                if seg_idx == 0 {
                    if is_bookmarked {
                        push_fg(out, pal.grad[2], true);
                        out.push('◆');
                    } else if is_cur_line {
                        push_fg(out, pal.active_line_num, true);
                        out.push('▸');
                    } else {
                        out.push(' ');
                    }
                    if is_cur_line {
                        push_fg(out, pal.active_line_num, true);
                        let _ = write!(out, " {:>width$} ", doc_row + 1, width = num_digits);
                    } else if cfg.relative_numbers {
                        let dist = doc_row.abs_diff(primary_row);
                        push_fg(out, pal.line_num, false);
                        let _ = write!(out, " {:>width$} ", dist, width = num_digits);
                    } else {
                        push_fg(out, pal.line_num, false);
                        let _ = write!(out, " {:>width$} ", doc_row + 1, width = num_digits);
                    }
                } else {
                    push_fg(out, pal.dim, false);
                    let _ = write!(out, "  {:>width$} ", "↪", width = num_digits);
                }

                match git_marks.get(&doc_row) {
                    Some(GutterMark::Added) if seg_idx == 0 => {
                        push_fg(out, pal.ok, false);
                        out.push_str("▎ ");
                    }
                    Some(GutterMark::Modified) if seg_idx == 0 => {
                        push_fg(out, pal.warn, false);
                        out.push_str("▎ ");
                    }
                    Some(GutterMark::Deleted) if seg_idx == 0 => {
                        push_fg(out, pal.err, false);
                        out.push_str("▁ ");
                    }
                    _ => {
                        push_fg(out, pal.border, false);
                        out.push_str("│ ");
                    }
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

                let bg_c = if is_sec_caret {
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
                    out.push_str("\x1b[49m");
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

                if is_cur_line || is_ruler {
                    push_bg(out, pal.cur_line_bg);
                } else {
                    out.push_str("\x1b[49m");
                }
                if is_blank_guide {
                    let in_active_scope = active_scope.is_some_and(|(s_r, e_r, a_col)| {
                        doc_row >= s_r && doc_row <= e_r && vis_x == a_col
                    });
                    push_fg(out, if in_active_scope { pal.indent_active } else { pal.indent_guide }, false);
                    out.push('▏');
                } else {
                    out.push(' ');
                }
                drawn_cols += 1;
            }

            if show_sb {
                out.push_str("\x1b[49m");
                push_scrollbar_cell(out, screen_y, content_h, sb_start, sb_end, sb_cursor_row, pal);
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
    pal: &Palette,
) {
    let t = screen_y as f32 / content_h.saturating_sub(1).max(1) as f32;
    if screen_y == sb_cursor_row {
        push_fg(out, pal.grad[0], true);
        out.push('█');
    } else if screen_y >= sb_start && screen_y < sb_end {
        push_fg(out, lerp_grad(pal.grad, t), false);
        out.push('┃');
    } else {
        push_fg(out, pal.border, false);
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
