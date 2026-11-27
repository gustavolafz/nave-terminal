use crate::palette::{lerp_grad, push_bg, push_fg_styled, Palette, Rgb};
use crate::rope::Rope;
use crate::syntax::{lex_line, Language, LineState};
use std::fmt::Write as _;

#[derive(Clone, Debug)]
pub struct MdSpan {
    pub text: String,
    pub fg: Rgb,
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub gradient: bool,
}

impl MdSpan {
    pub fn plain(text: impl Into<String>, fg: Rgb) -> Self {
        Self {
            text: text.into(),
            fg,
            bg: None,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            gradient: false,
        }
    }

    pub fn styled(text: impl Into<String>, fg: Rgb, bold: bool, italic: bool) -> Self {
        Self {
            text: text.into(),
            fg,
            bg: None,
            bold,
            italic,
            underline: false,
            strikethrough: false,
            gradient: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MdLine {
    pub spans: Vec<MdSpan>,
    pub line_bg: Option<Rgb>,
    pub source_line: usize,
    pub heading_title: Option<String>,
}

impl MdLine {
    pub fn blank(source_line: usize) -> Self {
        Self {
            spans: Vec::new(),
            line_bg: None,
            source_line,
            heading_title: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RenderedMarkdown {
    pub lines: Vec<MdLine>,
    pub heading_rows: Vec<usize>,
    pub left_margin: usize,
}

impl RenderedMarkdown {
    pub fn total_lines(&self) -> usize {
        self.lines.len().max(1)
    }

    /// Find the first visual row corresponding to `source_line` (or nearest preceding).
    pub fn visual_row_for_source(&self, source_line: usize) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        for (idx, ln) in self.lines.iter().enumerate() {
            if ln.source_line >= source_line {
                return idx;
            }
        }
        self.lines.len().saturating_sub(1)
    }

    /// Find the next or previous heading visual row from `cur_row`.
    pub fn jump_heading(&self, cur_row: usize, forward: bool) -> Option<(usize, String)> {
        if self.heading_rows.is_empty() {
            return None;
        }
        let target = if forward {
            self.heading_rows
                .iter()
                .copied()
                .find(|&r| r > cur_row)
                .or_else(|| self.heading_rows.first().copied())?
        } else {
            self.heading_rows
                .iter()
                .rev()
                .copied()
                .find(|&r| r < cur_row)
                .or_else(|| self.heading_rows.last().copied())?
        };
        let title = self
            .lines
            .get(target)
            .and_then(|l| l.heading_title.clone())
            .unwrap_or_default();
        Some((target, title))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug)]
struct InlineCtx {
    fg: Rgb,
    bold: bool,
    italic: bool,
    strikethrough: bool,
}

#[inline]
pub fn md_char_width(ch: char) -> usize {
    let cp = ch as u32;
    if cp == 0 || (0x0300..=0x036F).contains(&cp) || (0xFE00..=0xFE0F).contains(&cp) || cp == 0x200D
    {
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

#[inline]
fn md_str_width(s: &str) -> usize {
    s.chars().map(md_char_width).sum()
}

fn spans_width(spans: &[MdSpan]) -> usize {
    spans.iter().map(|s| md_str_width(&s.text)).sum()
}

fn blend_rgb(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| -> u8 {
        ((x as f32) + ((y as f32) - (x as f32)) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

fn map_fence_language(tag: &str) -> Language {
    let t = tag
        .trim()
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match t.as_str() {
        "rs" | "rust" => Language::Rust,
        "py" | "python" | "python3" => Language::Python,
        "js" | "jsx" | "ts" | "tsx" | "javascript" | "typescript" => Language::JsTs,
        "sh" | "bash" | "zsh" | "shell" | "console" | "ps1" | "powershell" => Language::Shell,
        "json" | "jsonc" => Language::Json,
        "yml" | "yaml" => Language::Yaml,
        "toml" | "ini" | "conf" | "env" => Language::TomlIni,
        "md" | "markdown" => Language::Markdown,
        "dockerfile" | "docker" => Language::Dockerfile,
        "sql" | "postgres" | "mysql" | "sqlite" => Language::Sql,
        "c" | "h" | "cpp" | "c++" | "cc" | "cxx" | "hpp" | "cs" | "csharp" | "hlsl" => {
            Language::CLike
        }
        "java" => Language::Java,
        "go" | "golang" => Language::Go,
        "html" | "htm" | "xml" | "svg" | "css" | "scss" => Language::HtmlCss,
        _ => Language::Plain,
    }
}

/// Render a Markdown `Rope` into styled, word-wrapped visual lines tailored to `pane_text_w`.
pub fn render_markdown(rope: &Rope, pane_text_w: usize, pal: &Palette) -> RenderedMarkdown {
    let total_src = rope.line_count();
    let max_reader_w = 100usize;
    let (left_margin, col_w) = if pane_text_w >= max_reader_w + 6 {
        let m = (pane_text_w - max_reader_w) / 2;
        (m, max_reader_w)
    } else if pane_text_w >= 28 {
        (2, pane_text_w - 4)
    } else {
        (0, pane_text_w.max(8))
    };

    let raw_lines: Vec<String> = (0..total_src).map(|r| rope.line(r)).collect();
    let mut out: Vec<MdLine> = Vec::with_capacity(total_src * 2);
    let mut heading_rows: Vec<usize> = Vec::new();
    let mut i = 0usize;

    // 1. Optional YAML frontmatter at top of file
    if raw_lines.first().is_some_and(|l| l.trim() == "---") {
        if let Some(end_rel) = raw_lines[1..].iter().position(|l| {
            let t = l.trim();
            t == "---" || t == "..."
        }) {
            let end_idx = 1 + end_rel;
            let meta_bg = blend_rgb(pal.bg, pal.bar_bg, 0.72);
            let title = " metadata ";
            let inner = col_w.saturating_sub(2);
            let rule_n = inner.saturating_sub(md_str_width(title) + 1);
            out.push(MdLine {
                spans: vec![
                    MdSpan::plain("╭─", pal.border),
                    MdSpan::styled(title, pal.dim, true, false),
                    MdSpan::plain("─".repeat(rule_n), pal.border),
                    MdSpan::plain("╮", pal.border),
                ],
                line_bg: None,
                source_line: 0,
                heading_title: None,
            });
            for (offset, fl) in raw_lines[1..end_idx].iter().enumerate() {
                let src_ln = 1 + offset;
                let mut row_spans = vec![MdSpan::plain("│ ", pal.border)];
                if let Some((k, v)) = fl.split_once(':') {
                    row_spans.push(MdSpan::styled(k.trim(), pal.key, true, false));
                    row_spans.push(MdSpan::plain(": ", pal.dim));
                    row_spans.push(MdSpan::plain(v.trim(), pal.str_col));
                } else {
                    row_spans.push(MdSpan::plain(fl.trim(), pal.dim));
                }
                pad_box_row(&mut row_spans, col_w, pal.border, Some(meta_bg));
                out.push(MdLine {
                    spans: row_spans,
                    line_bg: Some(meta_bg),
                    source_line: src_ln,
                    heading_title: None,
                });
            }
            out.push(MdLine {
                spans: vec![
                    MdSpan::plain("╰", pal.border),
                    MdSpan::plain("─".repeat(inner), pal.border),
                    MdSpan::plain("╯", pal.border),
                ],
                line_bg: None,
                source_line: end_idx,
                heading_title: None,
            });
            out.push(MdLine::blank(end_idx));
            i = end_idx + 1;
        }
    }

    while i < raw_lines.len() {
        let line = &raw_lines[i];
        let trimmed = line.trim();

        // Blank line -> preserve max 1 consecutive blank separator
        if trimmed.is_empty() {
            if out.last().is_some_and(|l| !l.spans.is_empty()) {
                out.push(MdLine::blank(i));
            }
            i += 1;
            continue;
        }

        // Fenced code block (``` or ~~~)
        if let Some((fence_ch, fence_len, lang_tag)) = parse_fence_open(trimmed) {
            let start_src = i;
            i += 1;
            let mut code_lines: Vec<(usize, String)> = Vec::new();
            while i < raw_lines.len() {
                if is_fence_close(raw_lines[i].trim(), fence_ch, fence_len) {
                    i += 1;
                    break;
                }
                code_lines.push((i, raw_lines[i].clone()));
                i += 1;
            }
            render_code_block(&code_lines, &lang_tag, start_src, col_w, pal, &mut out);
            out.push(MdLine::blank(i.saturating_sub(1)));
            continue;
        }

        // ATX Heading (# .. ######)
        if let Some((level, title_raw)) = parse_atx_heading(trimmed) {
            render_heading(level, title_raw, i, col_w, pal, &mut out, &mut heading_rows);
            i += 1;
            continue;
        }

        // Setext Heading (next line is === or ---)
        if i + 1 < raw_lines.len() && !trimmed.starts_with('>') && !trimmed.starts_with('|') {
            let next_t = raw_lines[i + 1].trim();
            if is_setext_underline(next_t, '=') {
                render_heading(1, trimmed, i, col_w, pal, &mut out, &mut heading_rows);
                i += 2;
                continue;
            } else if is_setext_underline(next_t, '-')
                && parse_list_item(line).is_none()
            {
                render_heading(2, trimmed, i, col_w, pal, &mut out, &mut heading_rows);
                i += 2;
                continue;
            }
        }

        // Horizontal rule (---, ***, ___)
        if is_horizontal_rule(trimmed) {
            out.push(MdLine {
                spans: vec![MdSpan::plain("─".repeat(col_w), pal.border)],
                line_bg: None,
                source_line: i,
                heading_title: None,
            });
            out.push(MdLine::blank(i));
            i += 1;
            continue;
        }

        // GFM Table (header row + separator row `|---|---|`)
        if i + 1 < raw_lines.len() && looks_like_table_row(trimmed) {
            if let Some(aligns) = parse_table_separator(raw_lines[i + 1].trim()) {
                let start_src = i;
                let header_cells = split_table_cells(trimmed);
                i += 2;
                let mut body_rows: Vec<(usize, Vec<String>)> = Vec::new();
                while i < raw_lines.len() {
                    let rt = raw_lines[i].trim();
                    if rt.is_empty() || !looks_like_table_row(rt) {
                        break;
                    }
                    body_rows.push((i, split_table_cells(rt)));
                    i += 1;
                }
                render_table(
                    &header_cells,
                    &aligns,
                    &body_rows,
                    start_src,
                    col_w,
                    pal,
                    &mut out,
                );
                out.push(MdLine::blank(i.saturating_sub(1)));
                continue;
            }
        }

        // Blockquote / GitHub Alert Callout (`> ...`)
        if trimmed.starts_with('>') {
            let start_src = i;
            let mut q_lines: Vec<(usize, String)> = Vec::new();
            while i < raw_lines.len() {
                let qt = raw_lines[i].trim_start();
                if let Some(rest) = qt.strip_prefix('>') {
                    let stripped = rest.strip_prefix(' ').unwrap_or(rest);
                    q_lines.push((i, stripped.to_string()));
                    i += 1;
                } else {
                    break;
                }
            }
            render_blockquote(&q_lines, start_src, col_w, pal, &mut out);
            out.push(MdLine::blank(i.saturating_sub(1)));
            continue;
        }

        // List item (unordered, ordered, or task checklist)
        if let Some(item) = parse_list_item(line) {
            let start_src = i;
            let mut body_parts = vec![item.content.to_string()];
            i += 1;
            // Gather indented soft-continuation lines belonging to this list item
            while i < raw_lines.len() {
                let nl = &raw_lines[i];
                let nt = nl.trim();
                if nt.is_empty()
                    || parse_list_item(nl).is_some()
                    || parse_atx_heading(nt).is_some()
                    || parse_fence_open(nt).is_some()
                    || nt.starts_with('>')
                    || is_horizontal_rule(nt)
                    || looks_like_table_row(nt)
                {
                    break;
                }
                let lead = nl.chars().take_while(|&c| c == ' ' || c == '\t').count();
                if lead >= item.indent_spaces + 2 {
                    body_parts.push(nt.to_string());
                    i += 1;
                } else {
                    break;
                }
            }
            render_list_item(&item, &body_parts.join(" "), start_src, col_w, pal, &mut out);
            continue;
        }

        // Regular paragraph (soft-join consecutive prose lines)
        let start_src = i;
        let mut para_lines: Vec<String> = Vec::new();
        while i < raw_lines.len() {
            let pl = &raw_lines[i];
            let pt = pl.trim();
            if pt.is_empty()
                || parse_fence_open(pt).is_some()
                || parse_atx_heading(pt).is_some()
                || is_horizontal_rule(pt)
                || pt.starts_with('>')
                || parse_list_item(pl).is_some()
            {
                break;
            }
            // Stop before a GFM table
            if i + 1 < raw_lines.len()
                && looks_like_table_row(pt)
                && parse_table_separator(raw_lines[i + 1].trim()).is_some()
            {
                break;
            }
            // Stop before a Setext heading
            if !para_lines.is_empty()
                && i + 1 < raw_lines.len()
                && (is_setext_underline(raw_lines[i + 1].trim(), '=')
                    || is_setext_underline(raw_lines[i + 1].trim(), '-'))
            {
                break;
            }
            // Respect explicit Markdown hard break (two trailing spaces or trailing `\`)
            let hard_break = pl.ends_with("  ") || pt.ends_with('\\');
            let clean = pt.strip_suffix('\\').unwrap_or(pt).to_string();
            para_lines.push(clean);
            i += 1;
            if hard_break {
                break;
            }
        }
        let joined = para_lines.join(" ");
        let base_ctx = InlineCtx {
            fg: pal.fg,
            bold: false,
            italic: false,
            strikethrough: false,
        };
        let spans = parse_inline(&joined, base_ctx, pal);
        let wrapped = wrap_spans(&spans, col_w, &[]);
        for wl in wrapped {
            out.push(MdLine {
                spans: wl,
                line_bg: None,
                source_line: start_src,
                heading_title: None,
            });
        }
    }

    // Trim trailing blank lines
    while out.last().is_some_and(|l| l.spans.is_empty()) {
        out.pop();
    }
    if out.is_empty() {
        out.push(MdLine {
            spans: vec![MdSpan::styled(
                "(documento Markdown vazio — pressione 'e' ou Alt+P para editar)",
                pal.dim,
                false,
                true,
            )],
            line_bg: None,
            source_line: 0,
            heading_title: None,
        });
    }

    RenderedMarkdown {
        lines: out,
        heading_rows,
        left_margin,
    }
}

fn parse_fence_open(trimmed: &str) -> Option<(char, usize, String)> {
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|&c| c == first).count();
    if count < 3 {
        return None;
    }
    let rest = trimmed[count..].trim();
    if first == '`' && rest.contains('`') {
        return None;
    }
    Some((first, count, rest.to_string()))
}

fn is_fence_close(trimmed: &str, ch: char, min_len: usize) -> bool {
    let count = trimmed.chars().take_while(|&c| c == ch).count();
    count >= min_len && trimmed[count..].trim().is_empty()
}

fn parse_atx_heading(trimmed: &str) -> Option<(usize, &str)> {
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &trimmed[hashes..];
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    let title = rest.trim().trim_end_matches('#').trim();
    Some((hashes, title))
}

fn is_setext_underline(trimmed: &str, ch: char) -> bool {
    trimmed.len() >= 3 && trimmed.chars().all(|c| c == ch)
}

fn is_horizontal_rule(trimmed: &str) -> bool {
    if trimmed.len() < 3 {
        return false;
    }
    let mut kind: Option<char> = None;
    let mut count = 0usize;
    for ch in trimmed.chars() {
        if ch == ' ' || ch == '\t' {
            continue;
        }
        if matches!(ch, '-' | '*' | '_') {
            if let Some(k) = kind {
                if k != ch {
                    return false;
                }
            } else {
                kind = Some(ch);
            }
            count += 1;
        } else {
            return false;
        }
    }
    count >= 3
}

fn render_heading(
    level: usize,
    title_raw: &str,
    source_line: usize,
    col_w: usize,
    pal: &Palette,
    out: &mut Vec<MdLine>,
    heading_rows: &mut Vec<usize>,
) {
    if out.last().is_some_and(|l| !l.spans.is_empty()) {
        out.push(MdLine::blank(source_line));
    }
    let heading_idx = out.len();
    heading_rows.push(heading_idx);

    let (fg, bold, italic, prefix, is_grad) = match level {
        1 => (pal.grad[0], true, false, "", true),
        2 => (pal.grad[0], true, false, "", false),
        3 => (pal.grad[1], true, false, "▸ ", false),
        4 => (pal.func, true, false, "◆ ", false),
        5 => (pal.ty, true, false, "· ", false),
        _ => (pal.dim, true, true, "· ", false),
    };

    let ctx = InlineCtx {
        fg,
        bold,
        italic,
        strikethrough: false,
    };
    let mut spans = parse_inline(title_raw, ctx, pal);
    if is_grad {
        for sp in &mut spans {
            if sp.bg.is_none() {
                sp.gradient = true;
                sp.bold = true;
            }
        }
    }

    let plain_title: String = spans.iter().map(|s| s.text.as_str()).collect();
    let pref_spans: Vec<MdSpan> = if prefix.is_empty() {
        Vec::new()
    } else {
        vec![MdSpan::styled(prefix, fg, true, false)]
    };
    let pref_w = spans_width(&pref_spans);
    let cont_pref = if pref_w > 0 {
        vec![MdSpan::plain(" ".repeat(pref_w), fg)]
    } else {
        Vec::new()
    };

    let wrapped = wrap_spans(&spans, col_w.saturating_sub(pref_w).max(4), &[]);
    for (wi, mut wl) in wrapped.into_iter().enumerate() {
        let mut row = if wi == 0 {
            pref_spans.clone()
        } else {
            cont_pref.clone()
        };
        row.append(&mut wl);
        out.push(MdLine {
            spans: row,
            line_bg: None,
            source_line,
            heading_title: if wi == 0 {
                Some(format!("{} {}", "#".repeat(level), plain_title))
            } else {
                None
            },
        });
    }

    if level == 1 {
        let rule_w = col_w;
        out.push(MdLine {
            spans: vec![MdSpan {
                text: "═".repeat(rule_w),
                fg: pal.grad[0],
                bg: None,
                bold: false,
                italic: false,
                underline: false,
                strikethrough: false,
                gradient: true,
            }],
            line_bg: None,
            source_line,
            heading_title: None,
        });
    } else if level == 2 {
        out.push(MdLine {
            spans: vec![MdSpan::plain("─".repeat(col_w), pal.border)],
            line_bg: None,
            source_line,
            heading_title: None,
        });
    }
    out.push(MdLine::blank(source_line));
}

fn pad_box_row(spans: &mut Vec<MdSpan>, box_w: usize, border_fg: Rgb, bg: Option<Rgb>) {
    let used = spans_width(spans);
    let inner_end = box_w.saturating_sub(1);
    if used < inner_end {
        let mut pad = MdSpan::plain(" ".repeat(inner_end - used), border_fg);
        pad.bg = bg;
        spans.push(pad);
    }
    let mut right_bar = MdSpan::plain("│", border_fg);
    right_bar.bg = bg;
    spans.push(right_bar);
}

fn render_code_block(
    code_lines: &[(usize, String)],
    lang_tag: &str,
    start_src: usize,
    col_w: usize,
    pal: &Palette,
    out: &mut Vec<MdLine>,
) {
    let lang = map_fence_language(lang_tag);
    let label_name = if lang_tag.trim().is_empty() {
        "code".to_string()
    } else {
        lang_tag.trim().split_whitespace().next().unwrap_or("code").to_string()
    };
    let code_bg = blend_rgb(pal.bg, pal.cur_line_bg, 0.85);
    let box_w = col_w.max(12);
    let inner_w = box_w.saturating_sub(2);
    let header_badge = format!(" {} ", label_name);
    let badge_w = md_str_width(&header_badge);
    let right_dashes = inner_w.saturating_sub(badge_w + 1);

    out.push(MdLine {
        spans: vec![
            MdSpan::plain("╭─", pal.border),
            MdSpan::styled(header_badge, pal.grad[0], true, false),
            MdSpan::plain("─".repeat(right_dashes), pal.border),
            MdSpan::plain("╮", pal.border),
        ],
        line_bg: None,
        source_line: start_src,
        heading_title: None,
    });

    let code_inner_w = box_w.saturating_sub(4).max(4); // `│ ` + code + ` │`
    let mut hl_state = LineState::NORMAL;

    if code_lines.is_empty() {
        let mut row = vec![MdSpan {
            text: "│ ".into(),
            fg: pal.border,
            bg: Some(code_bg),
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            gradient: false,
        }];
        pad_box_row(&mut row, box_w, pal.border, Some(code_bg));
        out.push(MdLine {
            spans: row,
            line_bg: Some(code_bg),
            source_line: start_src,
            heading_title: None,
        });
    } else {
        for (src_ln, raw) in code_lines {
            // Expand tabs to 4 spaces for deterministic column width
            let expanded = raw.replace('\t', "    ");
            let (hl_spans, next_st) = lex_line(lang, &expanded, hl_state);
            hl_state = next_st;

            // Convert syntax spans into styled MdSpans
            let mut styled_line: Vec<MdSpan> = Vec::new();
            let mut pos = 0usize;
            for sp in hl_spans {
                if sp.start > pos && sp.start <= expanded.len() {
                    styled_line.push(MdSpan {
                        text: expanded[pos..sp.start].to_string(),
                        fg: pal.fg,
                        bg: Some(code_bg),
                        bold: false,
                        italic: false,
                        underline: false,
                        strikethrough: false,
                        gradient: false,
                    });
                }
                let s = sp.start.min(expanded.len());
                let e = sp.end.min(expanded.len());
                if s < e {
                    let st = pal.token_style(sp.kind);
                    styled_line.push(MdSpan {
                        text: expanded[s..e].to_string(),
                        fg: st.fg,
                        bg: st.bg.or(Some(code_bg)),
                        bold: st.bold,
                        italic: st.italic,
                        underline: false,
                        strikethrough: false,
                        gradient: false,
                    });
                    pos = e;
                }
            }
            if pos < expanded.len() {
                styled_line.push(MdSpan {
                    text: expanded[pos..].to_string(),
                    fg: pal.fg,
                    bg: Some(code_bg),
                    bold: false,
                    italic: false,
                    underline: false,
                    strikethrough: false,
                    gradient: false,
                });
            }

            let wrapped = wrap_code_spans(&styled_line, code_inner_w, code_bg, pal.dim);
            for mut wline in wrapped {
                let mut row = vec![MdSpan {
                    text: "│ ".into(),
                    fg: pal.border,
                    bg: Some(code_bg),
                    bold: false,
                    italic: false,
                    underline: false,
                    strikethrough: false,
                    gradient: false,
                }];
                row.append(&mut wline);
                pad_box_row(&mut row, box_w, pal.border, Some(code_bg));
                out.push(MdLine {
                    spans: row,
                    line_bg: Some(code_bg),
                    source_line: *src_ln,
                    heading_title: None,
                });
            }
        }
    }

    let end_src = code_lines.last().map(|(l, _)| *l).unwrap_or(start_src);
    out.push(MdLine {
        spans: vec![
            MdSpan::plain("╰", pal.border),
            MdSpan::plain("─".repeat(inner_w), pal.border),
            MdSpan::plain("╯", pal.border),
        ],
        line_bg: None,
        source_line: end_src,
        heading_title: None,
    });
}

fn wrap_code_spans(
    spans: &[MdSpan],
    max_w: usize,
    code_bg: Rgb,
    dim_fg: Rgb,
) -> Vec<Vec<MdSpan>> {
    if spans.is_empty() {
        return vec![Vec::new()];
    }
    let mut rows: Vec<Vec<MdSpan>> = Vec::new();
    let mut cur: Vec<MdSpan> = Vec::new();
    let mut cur_w = 0usize;

    for sp in spans {
        let mut seg = String::new();
        for ch in sp.text.chars() {
            let cw = md_char_width(ch);
            if cur_w + cw > max_w && cur_w > 0 {
                if !seg.is_empty() {
                    let mut s2 = sp.clone();
                    s2.text = std::mem::take(&mut seg);
                    cur.push(s2);
                }
                rows.push(std::mem::take(&mut cur));
                cur.push(MdSpan {
                    text: "↪ ".into(),
                    fg: dim_fg,
                    bg: Some(code_bg),
                    bold: false,
                    italic: false,
                    underline: false,
                    strikethrough: false,
                    gradient: false,
                });
                cur_w = 2;
            }
            seg.push(ch);
            cur_w += cw;
        }
        if !seg.is_empty() {
            let mut s2 = sp.clone();
            s2.text = seg;
            cur.push(s2);
        }
    }
    rows.push(cur);
    rows
}

fn looks_like_table_row(trimmed: &str) -> bool {
    trimmed.contains('|') && trimmed.len() >= 3
}

fn split_table_cells(trimmed: &str) -> Vec<String> {
    let s = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let s = s.strip_suffix('|').unwrap_or(s);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek() == Some(&'|') {
            cur.push('|');
            chars.next();
        } else if ch == '|' {
            cells.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(ch);
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

fn parse_table_separator(trimmed: &str) -> Option<Vec<Align>> {
    if !trimmed.contains('|') && !trimmed.contains('-') {
        return None;
    }
    let cells = split_table_cells(trimmed);
    if cells.is_empty() {
        return None;
    }
    let mut aligns = Vec::with_capacity(cells.len());
    for c in cells {
        let ct = c.trim();
        if ct.is_empty() {
            return None;
        }
        let left = ct.starts_with(':');
        let right = ct.ends_with(':');
        let inner = ct.trim_matches(':');
        if inner.is_empty() || !inner.chars().all(|ch| ch == '-') {
            return None;
        }
        aligns.push(match (left, right) {
            (true, true) => Align::Center,
            (false, true) => Align::Right,
            _ => Align::Left,
        });
    }
    Some(aligns)
}

fn render_table(
    header_raw: &[String],
    aligns: &[Align],
    body_rows: &[(usize, Vec<String>)],
    start_src: usize,
    col_w: usize,
    pal: &Palette,
    out: &mut Vec<MdLine>,
) {
    let ncols = aligns.len().max(header_raw.len()).max(1);
    let hdr_ctx = InlineCtx {
        fg: pal.grad[0],
        bold: true,
        italic: false,
        strikethrough: false,
    };
    let cell_ctx = InlineCtx {
        fg: pal.fg,
        bold: false,
        italic: false,
        strikethrough: false,
    };

    let hdr_spans: Vec<Vec<MdSpan>> = (0..ncols)
        .map(|c| {
            let txt = header_raw.get(c).map(String::as_str).unwrap_or("");
            parse_inline(txt, hdr_ctx, pal)
        })
        .collect();
    let body_spans: Vec<(usize, Vec<Vec<MdSpan>>)> = body_rows
        .iter()
        .map(|(src_ln, row)| {
            let cells = (0..ncols)
                .map(|c| {
                    let txt = row.get(c).map(String::as_str).unwrap_or("");
                    parse_inline(txt, cell_ctx, pal)
                })
                .collect();
            (*src_ln, cells)
        })
        .collect();

    // Measure natural widths per column
    let mut widths: Vec<usize> = vec![3; ncols];
    for (c, sp) in hdr_spans.iter().enumerate() {
        widths[c] = widths[c].max(spans_width(sp));
    }
    for (_, row) in &body_spans {
        for (c, sp) in row.iter().enumerate() {
            widths[c] = widths[c].max(spans_width(sp));
        }
    }

    // Budget widths to fit `col_w` (overhead: `│ ` + ` │ `*(n-1) + ` │` = 3*ncols + 1)
    let overhead = 3 * ncols + 1;
    let avail = col_w.saturating_sub(overhead).max(ncols * 3);
    let sum_w: usize = widths.iter().sum();
    if sum_w > avail {
        // Proportionally shrink widest columns while keeping minimum 4 chars per col
        let mut remaining = sum_w - avail;
        while remaining > 0 {
            let mut max_idx = 0usize;
            for c in 1..ncols {
                if widths[c] > widths[max_idx] {
                    max_idx = c;
                }
            }
            if widths[max_idx] <= 4 {
                break;
            }
            widths[max_idx] -= 1;
            remaining -= 1;
        }
    }

    let make_border = |l: char, m: char, r: char| -> MdLine {
        let mut s = String::new();
        s.push(l);
        for (idx, &w) in widths.iter().enumerate() {
            for _ in 0..(w + 2) {
                s.push('─');
            }
            if idx + 1 < ncols {
                s.push(m);
            }
        }
        s.push(r);
        MdLine {
            spans: vec![MdSpan::plain(s, pal.border)],
            line_bg: None,
            source_line: start_src,
            heading_title: None,
        }
    };

    let push_table_row = |cells: &[Vec<MdSpan>],
                          src_ln: usize,
                          row_bg: Option<Rgb>,
                          out: &mut Vec<MdLine>| {
        let wrapped_cols: Vec<Vec<Vec<MdSpan>>> = (0..ncols)
            .map(|c| {
                let mut w = wrap_spans(&cells[c], widths[c], &[]);
                if w.is_empty() {
                    w.push(Vec::new());
                }
                w
            })
            .collect();
        let row_h = wrapped_cols.iter().map(|col| col.len()).max().unwrap_or(1);

        for sub in 0..row_h {
            let mut row_spans: Vec<MdSpan> = Vec::new();
            row_spans.push(MdSpan {
                text: "│".into(),
                fg: pal.border,
                bg: row_bg,
                bold: false,
                italic: false,
                underline: false,
                strikethrough: false,
                gradient: false,
            });
            for c in 0..ncols {
                let cw = widths[c];
                let cell_line = wrapped_cols[c].get(sub).cloned().unwrap_or_default();
                let used = spans_width(&cell_line);
                let pad_total = cw.saturating_sub(used);
                let align = aligns.get(c).copied().unwrap_or(Align::Left);
                let (pad_l, pad_r) = match align {
                    Align::Left => (0, pad_total),
                    Align::Right => (pad_total, 0),
                    Align::Center => (pad_total / 2, pad_total - pad_total / 2),
                };
                row_spans.push(MdSpan {
                    text: " ".repeat(1 + pad_l),
                    fg: pal.fg,
                    bg: row_bg,
                    bold: false,
                    italic: false,
                    underline: false,
                    strikethrough: false,
                    gradient: false,
                });
                for mut sp in cell_line {
                    if sp.bg.is_none() {
                        sp.bg = row_bg;
                    }
                    row_spans.push(sp);
                }
                row_spans.push(MdSpan {
                    text: " ".repeat(pad_r + 1),
                    fg: pal.fg,
                    bg: row_bg,
                    bold: false,
                    italic: false,
                    underline: false,
                    strikethrough: false,
                    gradient: false,
                });
                row_spans.push(MdSpan {
                    text: "│".into(),
                    fg: pal.border,
                    bg: row_bg,
                    bold: false,
                    italic: false,
                    underline: false,
                    strikethrough: false,
                    gradient: false,
                });
            }
            out.push(MdLine {
                spans: row_spans,
                line_bg: row_bg,
                source_line: src_ln,
                heading_title: None,
            });
        }
    };

    let hdr_bg = Some(blend_rgb(pal.bg, pal.bar_bg, 0.85));
    out.push(make_border('┌', '┬', '┐'));
    push_table_row(&hdr_spans, start_src, hdr_bg, out);
    out.push(make_border('├', '┼', '┤'));
    for (src_ln, row_cells) in &body_spans {
        push_table_row(row_cells, *src_ln, None, out);
    }
    let mut bottom = make_border('└', '┴', '┘');
    if let Some((last_src, _)) = body_spans.last() {
        bottom.source_line = *last_src;
    }
    out.push(bottom);
}

fn render_blockquote(
    q_lines: &[(usize, String)],
    start_src: usize,
    col_w: usize,
    pal: &Palette,
    out: &mut Vec<MdLine>,
) {
    // Detect GitHub-style Alerts: [!NOTE], [!TIP], [!IMPORTANT], [!WARNING], [!CAUTION]
    let alert_info: Option<(&'static str, Rgb)> = q_lines.first().and_then(|(_, first)| {
        match first.trim().to_ascii_uppercase().as_str() {
            "[!NOTE]" => Some(("◈ NOTE", pal.grad[0])),
            "[!TIP]" => Some(("✓ TIP", pal.ok)),
            "[!IMPORTANT]" => Some(("★ IMPORTANT", pal.grad[1])),
            "[!WARNING]" => Some(("⚠ WARNING", pal.warn)),
            "[!CAUTION]" => Some(("✖ CAUTION", pal.err)),
            _ => None,
        }
    });

    let (bar_color, bg_tint, body_start_idx) = if let Some((badge, color)) = alert_info {
        let tint = Some(blend_rgb(pal.bg, color, 0.08));
        out.push(MdLine {
            spans: vec![
                MdSpan {
                    text: "┃ ".into(),
                    fg: color,
                    bg: tint,
                    bold: true,
                    italic: false,
                    underline: false,
                    strikethrough: false,
                    gradient: false,
                },
                MdSpan {
                    text: badge.into(),
                    fg: color,
                    bg: tint,
                    bold: true,
                    italic: false,
                    underline: false,
                    strikethrough: false,
                    gradient: false,
                },
            ],
            line_bg: tint,
            source_line: start_src,
            heading_title: None,
        });
        (color, tint, 1usize)
    } else {
        (pal.grad[1], None, 0usize)
    };

    let body_w = col_w.saturating_sub(2).max(6);
    let ctx = InlineCtx {
        fg: if alert_info.is_some() { pal.fg } else { blend_rgb(pal.fg, pal.dim, 0.25) },
        bold: false,
        italic: alert_info.is_none(),
        strikethrough: false,
    };

    // Group non-empty blockquote lines into paragraphs
    let mut idx = body_start_idx;
    while idx < q_lines.len() {
        if q_lines[idx].1.trim().is_empty() {
            out.push(MdLine {
                spans: vec![MdSpan {
                    text: "┃".into(),
                    fg: bar_color,
                    bg: bg_tint,
                    bold: true,
                    italic: false,
                    underline: false,
                    strikethrough: false,
                    gradient: false,
                }],
                line_bg: bg_tint,
                source_line: q_lines[idx].0,
                heading_title: None,
            });
            idx += 1;
            continue;
        }
        let para_src = q_lines[idx].0;
        let mut parts = Vec::new();
        while idx < q_lines.len() && !q_lines[idx].1.trim().is_empty() {
            parts.push(q_lines[idx].1.trim().to_string());
            idx += 1;
        }
        let spans = parse_inline(&parts.join(" "), ctx, pal);
        let wrapped = wrap_spans(&spans, body_w, &[]);
        for mut wl in wrapped {
            let mut row = vec![MdSpan {
                text: "┃ ".into(),
                fg: bar_color,
                bg: bg_tint,
                bold: true,
                italic: false,
                underline: false,
                strikethrough: false,
                gradient: false,
            }];
            for sp in &mut wl {
                if sp.bg.is_none() {
                    sp.bg = bg_tint;
                }
            }
            row.append(&mut wl);
            out.push(MdLine {
                spans: row,
                line_bg: bg_tint,
                source_line: para_src,
                heading_title: None,
            });
        }
    }
}

#[derive(Clone, Debug)]
struct ListItem<'a> {
    indent_spaces: usize,
    kind: ListKind,
    content: &'a str,
}

#[derive(Clone, Debug)]
enum ListKind {
    Unordered,
    Ordered(String),
    Task(bool),
}

fn parse_list_item(line: &str) -> Option<ListItem<'_>> {
    let mut indent_spaces = 0usize;
    let mut byte_idx = 0usize;
    for (bi, ch) in line.char_indices() {
        if ch == ' ' {
            indent_spaces += 1;
            byte_idx = bi + 1;
        } else if ch == '\t' {
            indent_spaces += 2;
            byte_idx = bi + 1;
        } else {
            break;
        }
    }
    let rest = &line[byte_idx..];

    // Unordered: `- `, `* `, `+ `
    let first = rest.chars().next()?;
    if matches!(first, '-' | '*' | '+') {
        let after = &rest[1..];
        if let Some(body) = after.strip_prefix(' ').or_else(|| after.strip_prefix('\t')) {
            if let Some(task_rest) = body
                .strip_prefix("[x] ")
                .or_else(|| body.strip_prefix("[X] "))
            {
                return Some(ListItem {
                    indent_spaces,
                    kind: ListKind::Task(true),
                    content: task_rest,
                });
            }
            if let Some(task_rest) = body.strip_prefix("[ ] ") {
                return Some(ListItem {
                    indent_spaces,
                    kind: ListKind::Task(false),
                    content: task_rest,
                });
            }
            return Some(ListItem {
                indent_spaces,
                kind: ListKind::Unordered,
                content: body,
            });
        }
    }

    // Ordered: `1. ` or `1) `
    let digits = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && digits <= 5 && rest.len() > digits + 1 {
        let sep = rest.as_bytes()[digits];
        let sp = rest.as_bytes()[digits + 1];
        if (sep == b'.' || sep == b')') && (sp == b' ' || sp == b'\t') {
            return Some(ListItem {
                indent_spaces,
                kind: ListKind::Ordered(format!("{}{}", &rest[..digits], sep as char)),
                content: &rest[digits + 2..],
            });
        }
    }

    None
}

fn render_list_item(
    item: &ListItem<'_>,
    joined_content: &str,
    source_line: usize,
    col_w: usize,
    pal: &Palette,
    out: &mut Vec<MdLine>,
) {
    let depth = (item.indent_spaces / 2).min(5);
    let lead_pad = "  ".repeat(depth);

    let (marker_span, text_ctx) = match &item.kind {
        ListKind::Unordered => {
            let (glyph, col) = match depth {
                0 => ("• ", pal.grad[0]),
                1 => ("◦ ", pal.grad[1]),
                _ => ("▸ ", pal.func),
            };
            (
                MdSpan::styled(glyph, col, true, false),
                InlineCtx {
                    fg: pal.fg,
                    bold: false,
                    italic: false,
                    strikethrough: false,
                },
            )
        }
        ListKind::Ordered(num) => (
            MdSpan::styled(format!("{} ", num), pal.num, true, false),
            InlineCtx {
                fg: pal.fg,
                bold: false,
                italic: false,
                strikethrough: false,
            },
        ),
        ListKind::Task(done) => {
            if *done {
                (
                    MdSpan::styled("[✓] ", pal.ok, true, false),
                    InlineCtx {
                        fg: pal.dim,
                        bold: false,
                        italic: false,
                        strikethrough: false,
                    },
                )
            } else {
                (
                    MdSpan::styled("[ ] ", pal.dim, false, false),
                    InlineCtx {
                        fg: pal.fg,
                        bold: false,
                        italic: false,
                        strikethrough: false,
                    },
                )
            }
        }
    };

    let marker_w = md_str_width(&lead_pad) + md_str_width(&marker_span.text);
    let body_w = col_w.saturating_sub(marker_w).max(6);
    let spans = parse_inline(joined_content, text_ctx, pal);
    let wrapped = wrap_spans(&spans, body_w, &[]);
    let hang_pad = " ".repeat(marker_w);

    for (wi, mut wl) in wrapped.into_iter().enumerate() {
        let mut row = Vec::new();
        if wi == 0 {
            if !lead_pad.is_empty() {
                row.push(MdSpan::plain(lead_pad.clone(), pal.fg));
            }
            row.push(marker_span.clone());
        } else {
            row.push(MdSpan::plain(hang_pad.clone(), pal.fg));
        }
        row.append(&mut wl);
        out.push(MdLine {
            spans: row,
            line_bg: None,
            source_line,
            heading_title: None,
        });
    }
}

/// Recursive inline Markdown parser (`code`, `**bold**`, `*italic*`, `~~strike~~`,
/// `[link](url)`, `![img](url)`, `<kbd>Key</kbd>`, backslash escapes).
fn parse_inline(text: &str, ctx: InlineCtx, pal: &Palette) -> Vec<MdSpan> {
    let mut out: Vec<MdSpan> = Vec::new();
    let mut buf = String::new();
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;

    let flush = |buf: &mut String, out: &mut Vec<MdSpan>| {
        if !buf.is_empty() {
            out.push(MdSpan {
                text: std::mem::take(buf),
                fg: ctx.fg,
                bg: None,
                bold: ctx.bold,
                italic: ctx.italic,
                underline: false,
                strikethrough: ctx.strikethrough,
                gradient: false,
            });
        }
    };

    while i < len {
        let b = bytes[i];

        // 1. Backslash escape
        if b == b'\\' && i + 1 < len {
            let next_ch = text[i + 1..].chars().next().unwrap();
            if matches!(
                next_ch,
                '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '-' | '.' | '!' | '|' | '~' | '<' | '>'
            ) {
                buf.push(next_ch);
                i += 1 + next_ch.len_utf8();
                continue;
            }
        }

        // 2. Inline code (`...` or ``...``)
        if b == b'`' {
            let ticks = bytes[i..].iter().take_while(|&&c| c == b'`').count();
            let delim = &text[i..i + ticks];
            if let Some(rel_end) = text[i + ticks..].find(delim) {
                flush(&mut buf, &mut out);
                let inner = text[i + ticks..i + ticks + rel_end].trim();
                out.push(MdSpan {
                    text: format!(" {} ", inner),
                    fg: pal.func,
                    bg: Some(pal.cur_line_bg),
                    bold: false,
                    italic: false,
                    underline: false,
                    strikethrough: ctx.strikethrough,
                    gradient: false,
                });
                i += ticks + rel_end + ticks;
                continue;
            }
        }

        // 3. Inline `<kbd>...</kbd>` keycap pill & `<br>`
        if b == b'<' {
            let rem = &text[i..];
            if let Some(after_open) = rem
                .strip_prefix("<kbd>")
                .or_else(|| rem.strip_prefix("<KBD>"))
            {
                if let Some(end_rel) = after_open
                    .find("</kbd>")
                    .or_else(|| after_open.find("</KBD>"))
                {
                    flush(&mut buf, &mut out);
                    let key_txt = after_open[..end_rel].trim();
                    out.push(MdSpan {
                        text: format!("[{}]", key_txt),
                        fg: pal.grad[0],
                        bg: Some(pal.bar_bg),
                        bold: true,
                        italic: false,
                        underline: false,
                        strikethrough: false,
                        gradient: false,
                    });
                    i += 5 + end_rel + 6;
                    continue;
                }
            }
            if rem.starts_with("<br>") || rem.starts_with("<br/>") || rem.starts_with("<br />") {
                let close = rem.find('>').unwrap_or(3);
                buf.push(' ');
                i += close + 1;
                continue;
            }
        }

        // 4. Image `![alt](url)`
        if b == b'!' && i + 1 < len && bytes[i + 1] == b'[' {
            if let Some((alt, _url, consumed)) = parse_bracket_link(&text[i + 1..]) {
                flush(&mut buf, &mut out);
                let label = if alt.trim().is_empty() { "image" } else { alt.trim() };
                out.push(MdSpan {
                    text: format!("[img: {}]", label),
                    fg: pal.ty,
                    bg: None,
                    bold: false,
                    italic: true,
                    underline: false,
                    strikethrough: ctx.strikethrough,
                    gradient: false,
                });
                i += 1 + consumed;
                continue;
            }
        }

        // 5. Link `[label](url)`
        if b == b'[' {
            if let Some((label, url, consumed)) = parse_bracket_link(&text[i..]) {
                flush(&mut buf, &mut out);
                let link_ctx = InlineCtx {
                    fg: pal.grad[0],
                    bold: ctx.bold,
                    italic: ctx.italic,
                    strikethrough: ctx.strikethrough,
                };
                let mut inner_spans = parse_inline(label, link_ctx, pal);
                for sp in &mut inner_spans {
                    sp.underline = true;
                }
                out.append(&mut inner_spans);
                if url.starts_with("http://") || url.starts_with("https://") {
                    out.push(MdSpan::plain(" ↗", pal.dim));
                }
                i += consumed;
                continue;
            }
        }

        // 6. Strikethrough `~~...~~`
        if b == b'~' && i + 1 < len && bytes[i + 1] == b'~' {
            if let Some(rel_end) = text[i + 2..].find("~~") {
                if rel_end > 0 {
                    flush(&mut buf, &mut out);
                    let inner_ctx = InlineCtx {
                        fg: pal.dim,
                        bold: ctx.bold,
                        italic: ctx.italic,
                        strikethrough: true,
                    };
                    out.extend(parse_inline(&text[i + 2..i + 2 + rel_end], inner_ctx, pal));
                    i += 2 + rel_end + 2;
                    continue;
                }
            }
        }

        // 7. Bold+Italic `***...***`, Bold `**...**` / `__...__`, Italic `*...*` / `_..._`
        if b == b'*' || b == b'_' {
            // Avoid turning `snake_case_identifier` italic mid-word!
            let prev_is_alnum = i > 0
                && text[..i]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric());
            if b == b'_' && prev_is_alnum {
                buf.push('_');
                i += 1;
                continue;
            }
            let delim_ch = b as char;
            let count = bytes[i..].iter().take_while(|&&c| c == b).count().min(3);
            if count == 3 {
                let d3 = if b == b'*' { "***" } else { "___" };
                if let Some(rel_end) = find_closing_delim(&text[i + 3..], d3, b == b'_') {
                    flush(&mut buf, &mut out);
                    let inner_ctx = InlineCtx {
                        fg: ctx.fg,
                        bold: true,
                        italic: true,
                        strikethrough: ctx.strikethrough,
                    };
                    out.extend(parse_inline(&text[i + 3..i + 3 + rel_end], inner_ctx, pal));
                    i += 3 + rel_end + 3;
                    continue;
                }
            }
            if count >= 2 {
                let d2 = if b == b'*' { "**" } else { "__" };
                if let Some(rel_end) = find_closing_delim(&text[i + 2..], d2, b == b'_') {
                    flush(&mut buf, &mut out);
                    let inner_ctx = InlineCtx {
                        fg: ctx.fg,
                        bold: true,
                        italic: ctx.italic,
                        strikethrough: ctx.strikethrough,
                    };
                    out.extend(parse_inline(&text[i + 2..i + 2 + rel_end], inner_ctx, pal));
                    i += 2 + rel_end + 2;
                    continue;
                }
            }
            if count == 1 {
                let d1 = if b == b'*' { "*" } else { "_" };
                if !text[i + 1..].starts_with(' ') {
                    if let Some(rel_end) = find_closing_delim(&text[i + 1..], d1, b == b'_') {
                        if rel_end > 0 && !text[i + 1..i + 1 + rel_end].ends_with(' ') {
                            flush(&mut buf, &mut out);
                            let inner_ctx = InlineCtx {
                                fg: ctx.fg,
                                bold: ctx.bold,
                                italic: true,
                                strikethrough: ctx.strikethrough,
                            };
                            out.extend(parse_inline(&text[i + 1..i + 1 + rel_end], inner_ctx, pal));
                            i += 1 + rel_end + 1;
                            continue;
                        }
                    }
                }
            }
            buf.push(delim_ch);
            i += 1;
            continue;
        }

        let ch = text[i..].chars().next().unwrap();
        buf.push(ch);
        i += ch.len_utf8();
    }

    flush(&mut buf, &mut out);
    out
}

fn find_closing_delim(haystack: &str, delim: &str, check_word_boundary: bool) -> Option<usize> {
    let mut off = 0usize;
    while let Some(rel) = haystack[off..].find(delim) {
        let idx = off + rel;
        let after = idx + delim.len();
        let boundary_ok = if check_word_boundary {
            !haystack[after..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric())
        } else {
            true
        };
        if idx > 0 && boundary_ok {
            return Some(idx);
        }
        off = after;
    }
    None
}

fn parse_bracket_link(s: &str) -> Option<(&str, &str, usize)> {
    if !s.starts_with('[') {
        return None;
    }
    let mut depth = 0usize;
    let mut close_bracket = None;
    for (i, ch) in s.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    close_bracket = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let cb = close_bracket?;
    let after = &s[cb + 1..];
    if !after.starts_with('(') {
        return None;
    }
    let mut pdepth = 0usize;
    let mut close_paren = None;
    for (i, ch) in after.char_indices() {
        match ch {
            '(' => pdepth += 1,
            ')' => {
                pdepth = pdepth.saturating_sub(1);
                if pdepth == 0 {
                    close_paren = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let cp = close_paren?;
    let label = &s[1..cb];
    let url = after[1..cp].trim();
    Some((label, url, cb + 1 + cp + 1))
}

/// Word-wrap a sequence of styled `MdSpan`s cleanly to `max_w` display columns.
fn wrap_spans(spans: &[MdSpan], max_w: usize, cont_prefix: &[MdSpan]) -> Vec<Vec<MdSpan>> {
    let max_w = max_w.max(4);
    let mut words: Vec<(MdSpan, bool)> = Vec::new(); // (token_span, is_whitespace)

    for sp in spans {
        // Keep inline code pills (`bg.is_some()`) atomic when possible
        if sp.bg.is_some() && md_str_width(&sp.text) <= max_w {
            words.push((sp.clone(), false));
            continue;
        }
        let mut cur = String::new();
        let mut cur_ws: Option<bool> = None;
        for ch in sp.text.chars() {
            let ws = ch == ' ';
            if cur_ws.is_some_and(|prev| prev != ws) {
                let mut s2 = sp.clone();
                s2.text = std::mem::take(&mut cur);
                words.push((s2, cur_ws.unwrap()));
            }
            cur.push(ch);
            cur_ws = Some(ws);
        }
        if !cur.is_empty() {
            let mut s2 = sp.clone();
            s2.text = cur;
            words.push((s2, cur_ws.unwrap_or(false)));
        }
    }

    let mut rows: Vec<Vec<MdSpan>> = Vec::new();
    let mut cur_row: Vec<MdSpan> = Vec::new();
    let mut cur_w = 0usize;

    for (tok, is_ws) in words {
        let tw = md_str_width(&tok.text);
        if is_ws {
            if cur_w == 0 {
                continue; // never start wrapped line with stray spaces
            }
            if cur_w + tw <= max_w {
                cur_row.push(tok);
                cur_w += tw;
            } else {
                rows.push(std::mem::take(&mut cur_row));
                cur_row.extend(cont_prefix.iter().cloned());
                cur_w = spans_width(&cur_row);
            }
            continue;
        }

        if cur_w > 0 && cur_w + tw > max_w {
            // Trim trailing whitespace on finished row
            while cur_row.last().is_some_and(|s| s.text.chars().all(|c| c == ' ')) {
                cur_row.pop();
            }
            rows.push(std::mem::take(&mut cur_row));
            cur_row.extend(cont_prefix.iter().cloned());
            cur_w = spans_width(&cur_row);
        }

        if tw <= max_w.saturating_sub(cur_w) {
            cur_row.push(tok);
            cur_w += tw;
        } else {
            // Word wider than remaining width -> hard-break by display column cleanly
            let mut chunk = String::new();
            let mut cw_sum = 0usize;
            for ch in tok.text.chars() {
                let cw = md_char_width(ch);
                if cur_w + cw_sum + cw > max_w && (cur_w + cw_sum) > 0 {
                    if !chunk.is_empty() {
                        let mut s2 = tok.clone();
                        s2.text = std::mem::take(&mut chunk);
                        cur_row.push(s2);
                    }
                    rows.push(std::mem::take(&mut cur_row));
                    cur_row.extend(cont_prefix.iter().cloned());
                    cur_w = spans_width(&cur_row);
                    cw_sum = 0;
                }
                chunk.push(ch);
                cw_sum += cw;
            }
            if !chunk.is_empty() {
                let mut s2 = tok.clone();
                s2.text = chunk;
                cur_row.push(s2);
                cur_w += cw_sum;
            }
        }
    }

    while cur_row.last().is_some_and(|s| s.text.chars().all(|c| c == ' ')) {
        cur_row.pop();
    }
    if !cur_row.is_empty() || rows.is_empty() {
        rows.push(cur_row);
    }
    rows
}

/// Emit a single rendered Markdown line into `out` filling EXACTLY `text_w` columns.
pub fn write_md_line_cells(
    out: &mut String,
    line: Option<&MdLine>,
    left_margin: usize,
    text_w: usize,
    pal: &Palette,
    opaque_bg: bool,
) {
    let push_base = |out: &mut String| {
        if opaque_bg {
            push_bg(out, pal.bg);
        } else {
            out.push_str("\x1b[49m");
        }
    };

    let mut drawn = 0usize;
    let margin = left_margin.min(text_w);
    if margin > 0 {
        push_base(out);
        for _ in 0..margin {
            out.push(' ');
        }
        drawn += margin;
    }

    let Some(md_line) = line else {
        push_base(out);
        while drawn < text_w {
            out.push(' ');
            drawn += 1;
        }
        return;
    };

    // Total chars for smooth horizontal gradient on H1 titles/rules
    let grad_total: usize = md_line
        .spans
        .iter()
        .filter(|s| s.gradient)
        .map(|s| s.text.chars().count())
        .sum();
    let mut grad_idx = 0usize;

    for sp in &md_line.spans {
        if drawn >= text_w {
            break;
        }
        let eff_bg = sp.bg.or(md_line.line_bg);
        if let Some(bg) = eff_bg {
            push_bg(out, bg);
        } else {
            push_base(out);
        }

        if sp.gradient {
            let denom = grad_total.saturating_sub(1).max(1) as f32;
            for ch in sp.text.chars() {
                let cw = md_char_width(ch);
                if drawn + cw > text_w {
                    break;
                }
                let col = lerp_grad(pal.grad, grad_idx as f32 / denom);
                push_fg_styled(out, col, sp.bold, sp.italic);
                if sp.underline {
                    out.push_str("\x1b[4m");
                }
                if sp.strikethrough {
                    out.push_str("\x1b[9m");
                }
                out.push(ch);
                if sp.underline {
                    out.push_str("\x1b[24m");
                }
                if sp.strikethrough {
                    out.push_str("\x1b[29m");
                }
                drawn += cw;
                grad_idx += 1;
            }
        } else {
            push_fg_styled(out, sp.fg, sp.bold, sp.italic);
            if sp.underline {
                out.push_str("\x1b[4m");
            }
            if sp.strikethrough {
                out.push_str("\x1b[9m");
            }
            for ch in sp.text.chars() {
                let cw = md_char_width(ch);
                if drawn + cw > text_w {
                    break;
                }
                out.push(ch);
                drawn += cw;
            }
            if sp.underline {
                out.push_str("\x1b[24m");
            }
            if sp.strikethrough {
                out.push_str("\x1b[29m");
            }
        }
        // Reset bold/italic attributes cleanly before next span
        let _ = write!(out, "\x1b[22;23m");
    }

    push_base(out);
    while drawn < text_w {
        out.push(' ');
        drawn += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::PALETTES;

    #[test]
    fn renders_headings_tables_code_checklists_and_alerts() {
        let md = "\
# Guia DSA

> [!TIP]
> Use `long long` sempre!

| Comando | Acao |
| :--- | ---: |
| `gpush` | **Commit + Push** |

- [x] Feito com **negrito** e *italico*
- [ ] Pendente `snake_case_var`

```cpp
int main() { return 0; }
```
";
        let rope = Rope::from_str(md);
        let pal = &PALETTES[0];
        let rendered = render_markdown(&rope, 80, pal);

        assert!(!rendered.heading_rows.is_empty());
        assert_eq!(rendered.lines[rendered.heading_rows[0]].source_line, 0);

        let all_text: String = rendered
            .lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");

        // Headings, callout badge, table borders, checklist glyphs, fenced code box
        assert!(all_text.contains("Guia DSA"));
        assert!(all_text.contains("═══"));
        assert!(all_text.contains("✓ TIP"));
        assert!(all_text.contains("┌"));
        assert!(all_text.contains("┘"));
        assert!(all_text.contains("Commit + Push"));
        assert!(!all_text.contains("**Commit + Push**"));
        assert!(all_text.contains("[✓]"));
        assert!(all_text.contains("[ ]"));
        assert!(all_text.contains("snake_case_var"));
        assert!(all_text.contains("╭─ cpp"));
        assert!(all_text.contains("int main()"));
    }

    #[test]
    fn heading_jump_wraps_and_maps_source_lines() {
        let md = "# Intro\n\ntexto\n\n## Parte 2\n\nmais texto\n\n### Detalhe\n";
        let rope = Rope::from_str(md);
        let rendered = render_markdown(&rope, 60, &PALETTES[0]);

        assert_eq!(rendered.heading_rows.len(), 3);
        let h0 = rendered.heading_rows[0];
        let h1 = rendered.heading_rows[1];
        let h2 = rendered.heading_rows[2];

        assert_eq!(rendered.jump_heading(h0, true).unwrap().0, h1);
        assert_eq!(rendered.jump_heading(h1, true).unwrap().0, h2);
        assert_eq!(rendered.jump_heading(h2, true).unwrap().0, h0);
        assert_eq!(rendered.jump_heading(h0, false).unwrap().0, h2);
        assert_eq!(rendered.visual_row_for_source(4), h1);
    }
}
