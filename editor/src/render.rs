use crate::document::Document;
use crate::git::GutterMark;
use crate::palette::{push_bg, push_fg, push_grad_text, Palette, TokenKind};
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
pub struct LayoutInfo {
    pub gutter_width: usize,
    pub content_start_row: usize,
    pub visual_rows: Vec<VisualRowInfo>,
    pub tab_hitboxes: Vec<(usize, usize, usize)>, // (start_col, end_col, tab_idx)
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

pub fn ensure_cursor_visible(doc: &mut Document, cols: usize, rows: usize, wrap: bool, has_prompt: bool) {
    let reserved = if has_prompt { 3 } else { 2 };
    let view_h = rows.saturating_sub(reserved).max(1);
    let gutter_w = compute_gutter_width(doc.rope.line_count());
    let text_w = cols.saturating_sub(gutter_w).max(4);
    let (prow, pcol) = doc.primary_line_col();

    let scrolloff = 3.min(view_h / 3);
    if prow < doc.scroll_row + scrolloff {
        doc.scroll_row = prow.saturating_sub(scrolloff);
    } else if prow + scrolloff >= doc.scroll_row + view_h {
        doc.scroll_row = (prow + scrolloff + 1).saturating_sub(view_h);
    }

    if wrap {
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
    digits + 4 // " 123 ▎ "
}

pub fn render_editor(
    tabs: &[Document],
    active_tab: usize,
    doc: &mut Document,
    pal: &Palette,
    cols: usize,
    rows: usize,
    wrap: bool,
    prompt: PromptBar<'_>,
    picker: Option<&Picker>,
    status_msg: Option<&str>,
    synced: bool,
) -> (String, LayoutInfo) {
    let mut out = String::with_capacity(cols * rows * 4);
    if synced {
        out.push_str(SYNC_START);
    }
    out.push_str("\x1b[?25l\x1b[H");

    let has_prompt = !matches!(prompt, PromptBar::None);
    let reserved = if has_prompt { 3 } else { 2 };
    let content_h = rows.saturating_sub(reserved).max(1);
    let gutter_w = compute_gutter_width(doc.rope.line_count());
    let text_w = cols.saturating_sub(gutter_w).max(4);

    let mut layout = LayoutInfo {
        gutter_width: gutter_w,
        content_start_row: 1,
        visual_rows: Vec::with_capacity(content_h),
        tab_hitboxes: Vec::new(),
    };

    // -------------------------------------------------------------------------
    // Row 0: Header / Tab Bar
    // -------------------------------------------------------------------------
    push_bg(&mut out, pal.bar_bg);
    let brand = " NAVE EDIT ";
    push_grad_text(&mut out, brand, pal.grad, true);
    let mut used_cols = brand.chars().count();

    push_fg(&mut out, pal.border, false);
    out.push('│');
    used_cols += 1;

    let right_meta = format!(
        " [{} · {}{}] [{}] ",
        doc.lang.name(),
        doc.eol.label(),
        if wrap { " · WRAP" } else { "" },
        pal.name
    );
    let right_w = right_meta.chars().count();
    let max_tabs_col = cols.saturating_sub(right_w + 1);

    for (idx, t) in tabs.iter().enumerate() {
        let dirty = if idx == active_tab {
            doc.is_dirty()
        } else {
            t.is_dirty()
        };
        let name = if idx == active_tab {
            doc.display_name()
        } else {
            t.display_name()
        };
        let label = if dirty {
            format!(" {} ● ", name)
        } else {
            format!(" {} ", name)
        };
        let lw = label.chars().count();
        if used_cols + lw + 1 > max_tabs_col && idx > 0 {
            break;
        }
        let start_c = used_cols;
        if idx == active_tab {
            push_bg(&mut out, pal.tab_active_bg);
            push_fg(&mut out, pal.grad[0], true);
            out.push_str(&label);
            push_bg(&mut out, pal.bar_bg);
        } else {
            push_fg(&mut out, pal.dim, false);
            out.push_str(&label);
        }
        layout.tab_hitboxes.push((start_c, start_c + lw, idx));
        used_cols += lw;
        push_fg(&mut out, pal.border, false);
        out.push('│');
        used_cols += 1;
    }

    if used_cols + right_w <= cols {
        let pad = cols - used_cols - right_w;
        for _ in 0..pad {
            out.push(' ');
        }
        push_fg(&mut out, pal.dim, false);
        out.push_str(&right_meta);
    } else {
        out.push_str("\x1b[K");
    }
    out.push_str(RST);
    out.push_str("\r\n");

    // -------------------------------------------------------------------------
    // Editor Canvas (Rows 1 .. content_h)
    // -------------------------------------------------------------------------
    let (primary_row, primary_col) = doc.primary_line_col();
    let primary_head = doc.primary_cursor().head;
    let bracket_pair = find_matching_bracket(&doc.rope, primary_head);
    let git_marks: HashMap<usize, GutterMark> = doc.git.snapshot();
    let total_lines = doc.rope.line_count();
    let num_digits = gutter_w - 4;

    // Precompute selection ranges and secondary caret bytes
    let sel_ranges: Vec<(usize, usize)> = doc
        .cursors
        .iter()
        .filter(|c| !c.is_empty())
        .map(|c| c.range())
        .collect();
    let secondary_carets: Vec<usize> = doc
        .cursors
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != doc.primary)
        .map(|(_, c)| c.head)
        .collect();

    let mut primary_screen_pos: Option<(usize, usize)> = None;
    let mut screen_y = 0usize;
    let mut doc_row = doc.scroll_row;

    while screen_y < content_h {
        if doc_row >= total_lines {
            push_fg(&mut out, pal.border, false);
            let _ = write!(out, "{:>width$} │ ", "~", width = num_digits + 1);
            out.push_str(RST);
            out.push_str("\x1b[K");
            if screen_y + 1 < content_h || has_prompt || rows > 2 {
                out.push_str("\r\n");
            }
            screen_y += 1;
            continue;
        }

        let line_start = doc.rope.line_start_byte(doc_row);
        let line_text = doc.rope.line(doc_row);
        let spans = doc.syntax.line_spans(&doc.rope, doc_row);
        let is_cur_line = doc_row == primary_row;

        // Build per-char metadata once for this logical line
        let char_info = build_char_cells(&line_text, line_start, &spans, doc.tab_width);

        // Split into 1 visual segment (no wrap) or N segments (soft wrap)
        let segments: Vec<(usize, usize)> = if wrap {
            split_wrapped_segments(&char_info, text_w)
        } else {
            vec![(0, char_info.len())]
        };

        for (seg_idx, (seg_start, seg_end)) in segments.into_iter().enumerate() {
            if screen_y >= content_h {
                break;
            }
            layout.visual_rows.push(VisualRowInfo {
                doc_row,
                start_char_col: seg_start,
                end_char_col: seg_end,
            });

            // Gutter
            if is_cur_line {
                push_bg(&mut out, pal.cur_line_bg);
                push_fg(&mut out, pal.grad[0], true);
            } else {
                push_fg(&mut out, pal.dim, false);
            }
            if seg_idx == 0 {
                let _ = write!(out, " {:>width$} ", doc_row + 1, width = num_digits);
            } else {
                let _ = write!(out, " {:>width$} ", "↪", width = num_digits);
            }

            // Git gutter marker
            match git_marks.get(&doc_row) {
                Some(GutterMark::Added) if seg_idx == 0 => {
                    push_fg(&mut out, pal.ok, false);
                    out.push_str("▎ ");
                }
                Some(GutterMark::Modified) if seg_idx == 0 => {
                    push_fg(&mut out, pal.warn, false);
                    out.push_str("▎ ");
                }
                Some(GutterMark::Deleted) if seg_idx == 0 => {
                    push_fg(&mut out, pal.err, false);
                    out.push_str("▁ ");
                }
                _ => {
                    push_fg(&mut out, pal.border, false);
                    out.push_str("│ ");
                }
            }

            // Content cells
            let h_off = if wrap { 0 } else { doc.scroll_col };
            let mut cur_vis = 0usize;
            let mut drawn_cols = 0usize;

            // Check if primary cursor sits on this segment
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
                        primary_screen_pos = Some((1 + screen_y, gutter_w + (c_vis - h_off)));
                    }
                }
            }

            for cell in &char_info[seg_start..seg_end] {
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
                let is_active_match = doc
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

                let (mut fg_c, mut bold) = pal.token_color(cell.kind);
                let bg_c = if is_sec_caret {
                    fg_c = pal.bar_bg;
                    bold = true;
                    Some(pal.grad[0])
                } else if is_selected {
                    Some(pal.sel_bg)
                } else if is_active_match {
                    bold = true;
                    Some(pal.active_match_bg)
                } else if is_match {
                    Some(pal.match_bg)
                } else if is_bracket {
                    bold = true;
                    Some(pal.bracket_bg)
                } else if is_cur_line {
                    Some(pal.cur_line_bg)
                } else {
                    None
                };

                if let Some(bg) = bg_c {
                    push_bg(&mut out, bg);
                } else {
                    out.push_str("\x1b[49m");
                }
                push_fg(&mut out, fg_c, bold);

                if cell.ch == '\t' {
                    for _ in 0..cw {
                        out.push(' ');
                    }
                } else {
                    out.push(cell.ch);
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
                push_bg(&mut out, pal.grad[0]);
                push_fg(&mut out, pal.bar_bg, true);
                out.push(' ');
                drawn_cols += 1;
            }

            if is_cur_line && drawn_cols < text_w {
                push_bg(&mut out, pal.cur_line_bg);
                for _ in 0..(text_w - drawn_cols) {
                    out.push(' ');
                }
            } else {
                out.push_str("\x1b[49m");
            }
            out.push_str(RST);
            out.push_str("\x1b[K\r\n");
            screen_y += 1;
        }

        doc_row += 1;
    }

    // -------------------------------------------------------------------------
    // Optional Prompt Bar (Find / Replace / Goto / SaveAs / Confirm)
    // -------------------------------------------------------------------------
    let mut prompt_cursor_pos: Option<(usize, usize)> = None;
    let prompt_row = 1 + content_h;

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
            out.push_str("\r\n");
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
            out.push_str("\r\n");
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
            out.push_str("\r\n");
        }
        PromptBar::Confirm { message } => {
            push_bg(&mut out, pal.tab_active_bg);
            push_fg(&mut out, pal.warn, true);
            out.push_str(&format!(" {} ", message));
            out.push_str("\x1b[K");
            out.push_str(RST);
            out.push_str("\r\n");
        }
    }

    // -------------------------------------------------------------------------
    // Bottom Status Bar (Last Row)
    // -------------------------------------------------------------------------
    push_bg(&mut out, pal.bar_bg);
    let sel_chars: usize = doc
        .cursors
        .iter()
        .map(|c| {
            let (s, e) = c.range();
            doc.rope.slice(s, e).chars().count()
        })
        .sum();
    let multi_badge = if doc.cursors.len() > 1 {
        format!(" · {} cursores", doc.cursors.len())
    } else if sel_chars > 0 {
        format!(" · Sel {}", sel_chars)
    } else {
        String::new()
    };
    let left_status = format!(
        " Ln {}, Col {}{}",
        primary_row + 1,
        primary_col + 1,
        multi_badge
    );
    push_fg(&mut out, pal.grad[0], true);
    out.push_str(&left_status);

    let mut used_s = left_status.chars().count();
    if let Some(msg) = status_msg {
        let m = format!("  │  {}", msg);
        push_fg(&mut out, pal.ok, false);
        out.push_str(&truncate_to_cols(&m, cols.saturating_sub(used_s)));
        used_s += m.chars().count();
    } else if doc.external_changed {
        let m = "  │  ⚠ Alterado no disco (F5 recarregar)";
        push_fg(&mut out, pal.warn, true);
        out.push_str(&truncate_to_cols(m, cols.saturating_sub(used_s)));
        used_s += m.chars().count();
    }

    let right_help = "Ctrl+S salvar · Ctrl+F buscar · Ctrl+P abrir · F1 cmds · Ctrl+Q sair ";
    let rh_w = right_help.chars().count();
    if used_s + rh_w + 2 <= cols {
        let pad = cols - used_s - rh_w;
        for _ in 0..pad {
            out.push(' ');
        }
        push_fg(&mut out, pal.dim, false);
        out.push_str(right_help);
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
        let q_len = 3 + q_disp.chars().count();
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
                    item.detail.chars().count() + 2
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
                if !item.detail.is_empty() && written + item.detail.chars().count() + 1 <= inner_w {
                    push_fg(&mut out, pal.dim, false);
                    out.push_str(&item.detail);
                    out.push(' ');
                    written += item.detail.chars().count() + 1;
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
