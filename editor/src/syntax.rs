use crate::palette::TokenKind;
use crate::rope::Rope;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Rust,
    Python,
    JsTs,
    Shell,
    Json,
    Yaml,
    TomlIni,
    Markdown,
    Dockerfile,
    Sql,
    CLike,
    Go,
    HtmlCss,
    Plain,
}

impl Language {
    pub fn detect(path: Option<&Path>, first_line: &str) -> Self {
        if let Some(p) = path {
            let fname = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if fname == "dockerfile" || fname.starts_with("dockerfile.") || fname == "containerfile" {
                return Language::Dockerfile;
            }
            if fname.starts_with(".env") || fname == ".gitconfig" {
                return Language::TomlIni;
            }
            if matches!(
                fname.as_str(),
                ".zshrc" | ".bashrc" | ".profile" | ".zprofile" | ".zshenv" | "makefile"
            ) {
                return Language::Shell;
            }
            if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                return match ext.to_ascii_lowercase().as_str() {
                    "rs" => Language::Rust,
                    "py" | "pyi" => Language::Python,
                    "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Language::JsTs,
                    "sh" | "zsh" | "bash" | "zsh-theme" | "ps1" => Language::Shell,
                    "json" | "jsonc" => Language::Json,
                    "yml" | "yaml" => Language::Yaml,
                    "toml" | "ini" | "conf" | "cfg" | "env" => Language::TomlIni,
                    "md" | "markdown" => Language::Markdown,
                    "sql" => Language::Sql,
                    "c" | "h" | "cpp" | "cc" | "hpp" | "cs" | "java" | "hlsl" => Language::CLike,
                    "go" => Language::Go,
                    "html" | "htm" | "xml" | "svg" | "css" | "scss" => Language::HtmlCss,
                    _ => Self::from_shebang(first_line),
                };
            }
        }
        Self::from_shebang(first_line)
    }

    fn from_shebang(first_line: &str) -> Self {
        if let Some(rest) = first_line.strip_prefix("#!") {
            if rest.contains("python") {
                return Language::Python;
            }
            if rest.contains("bash") || rest.contains("zsh") || rest.contains("sh") {
                return Language::Shell;
            }
            if rest.contains("node") || rest.contains("deno") || rest.contains("bun") {
                return Language::JsTs;
            }
        }
        Language::Plain
    }

    pub fn name(self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::Python => "python",
            Language::JsTs => "js/ts",
            Language::Shell => "shell",
            Language::Json => "json",
            Language::Yaml => "yaml",
            Language::TomlIni => "toml/env",
            Language::Markdown => "markdown",
            Language::Dockerfile => "docker",
            Language::Sql => "sql",
            Language::CLike => "c/c#",
            Language::Go => "go",
            Language::HtmlCss => "html/css",
            Language::Plain => "text",
        }
    }

    pub fn comment_prefix(self) -> &'static str {
        match self {
            Language::Rust | Language::JsTs | Language::CLike | Language::Go | Language::Json => "//",
            Language::Sql => "--",
            _ => "#",
        }
    }

    pub fn indent_width(self) -> usize {
        match self {
            Language::Python | Language::Rust | Language::Go | Language::CLike => 4,
            _ => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineState {
    Normal,
    BlockComment,    // /* ... */
    HtmlComment,     // <!-- ... -->
    TripleDoubleStr, // """ ... """
    TripleSingleStr, // ''' ... '''
    BacktickStr,     // ` ... `
    MarkdownFence,   // ``` ... ```
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize, // byte offset within line
    pub end: usize,
    pub kind: TokenKind,
}

#[derive(Clone, Debug)]
struct CachedLine {
    in_state: LineState,
    out_state: LineState,
    text: String,
    spans: Vec<Span>,
}

pub struct SyntaxCache {
    lang: Language,
    lines: Vec<Option<CachedLine>>,
}

impl SyntaxCache {
    pub fn new(lang: Language) -> Self {
        Self {
            lang,
            lines: Vec::new(),
        }
    }

    pub fn set_language(&mut self, lang: Language) {
        if self.lang != lang {
            self.lang = lang;
            self.lines.clear();
        }
    }

    pub fn invalidate_from(&mut self, line: usize) {
        if line < self.lines.len() {
            for slot in &mut self.lines[line..] {
                *slot = None;
            }
        }
    }

    pub fn ensure_up_to(&mut self, rope: &Rope, target_line: usize) {
        let total = rope.line_count();
        if self.lines.len() != total {
            self.lines.resize(total, None);
        }
        let limit = target_line.min(total.saturating_sub(1));
        let mut state = LineState::Normal;

        for row in 0..=limit {
            if let Some(ref cached) = self.lines[row] {
                if cached.in_state == state {
                    state = cached.out_state;
                    continue;
                }
            }
            let text = rope.line(row);
            let (spans, out_state) = lex_line(self.lang, &text, state);
            self.lines[row] = Some(CachedLine {
                in_state: state,
                out_state,
                text,
                spans,
            });
            state = out_state;
        }
    }

    pub fn line_spans(&mut self, rope: &Rope, row: usize) -> Vec<Span> {
        self.ensure_up_to(rope, row);
        if let Some(Some(cached)) = self.lines.get(row) {
            let current = rope.line(row);
            if cached.text == current {
                return cached.spans.clone();
            }
            let in_state = cached.in_state;
            let (spans, out_state) = lex_line(self.lang, &current, in_state);
            let state_changed = out_state != cached.out_state;
            self.lines[row] = Some(CachedLine {
                in_state,
                out_state,
                text: current,
                spans: spans.clone(),
            });
            if state_changed {
                self.invalidate_from(row + 1);
            }
            spans
        } else {
            Vec::new()
        }
    }
}

pub fn lex_line(lang: Language, line: &str, mut state: LineState) -> (Vec<Span>, LineState) {
    if lang == Language::Plain || line.is_empty() {
        return (Vec::new(), state);
    }

    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut spans = Vec::new();
    let mut i = 0usize;

    // Special whole-line modes
    if lang == Language::Markdown {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            spans.push(Span { start: 0, end: len, kind: TokenKind::Function });
            let next = if state == LineState::MarkdownFence {
                LineState::Normal
            } else {
                LineState::MarkdownFence
            };
            return (spans, next);
        }
        if state == LineState::MarkdownFence {
            spans.push(Span { start: 0, end: len, kind: TokenKind::Function });
            return (spans, state);
        }
        if trimmed.starts_with('#') {
            spans.push(Span { start: 0, end: len, kind: TokenKind::Heading });
            return (spans, state);
        }
        if trimmed.starts_with('>') {
            spans.push(Span { start: 0, end: len, kind: TokenKind::String });
            return (spans, state);
        }
        // Highlight list markers (`- `, `* `) and inline code (`` `...` ``) in Markdown prose
        let lead = len - trimmed.len();
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            spans.push(Span { start: lead, end: lead + 1, kind: TokenKind::Type });
            i = lead + 2;
        }
        while i < len {
            if bytes[i] == b'`' {
                let start = i;
                i += 1;
                while i < len && bytes[i] != b'`' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                spans.push(Span { start, end: i, kind: TokenKind::Function });
                continue;
            }
            let ch_len = line[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            i += ch_len;
        }
        return (spans, state);
    }

    if lang == Language::TomlIni {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.starts_with(';') {
            spans.push(Span { start: 0, end: len, kind: TokenKind::Comment });
            return (spans, LineState::Normal);
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            spans.push(Span { start: 0, end: len, kind: TokenKind::Heading });
            return (spans, LineState::Normal);
        }
        if let Some(eq) = line.find('=') {
            let lead = len - trimmed.len();
            let kend = line[..eq].trim_end().len();
            if lead < kend {
                spans.push(Span { start: lead, end: kend, kind: TokenKind::Key });
            }
            spans.push(Span { start: eq, end: eq + 1, kind: TokenKind::Operator });
            i = eq + 1;
        }
    }

    if lang == Language::Yaml {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            spans.push(Span { start: 0, end: len, kind: TokenKind::Comment });
            return (spans, LineState::Normal);
        }
        if let Some(colon) = find_yaml_colon(line) {
            let lead = len - trimmed.len();
            let kstart = if trimmed.starts_with("- ") {
                spans.push(Span { start: lead, end: lead + 1, kind: TokenKind::Operator });
                lead + 2
            } else {
                lead
            };
            if kstart < colon {
                spans.push(Span { start: kstart, end: colon, kind: TokenKind::Key });
            }
            spans.push(Span { start: colon, end: colon + 1, kind: TokenKind::Operator });
            i = colon + 1;
        }
    }

    // Precompute parameter identifier ranges for function/method signatures on this line
    let param_ranges = if state == LineState::Normal {
        find_param_ranges(lang, line)
    } else {
        Vec::new()
    };

    let mut prev_word: &str = "";
    let mut prev_non_ws_byte: Option<u8> = None;
    let mut in_html_tag = false;
    let mut expect_html_tag_name = false;

    while i < len {
        // Handle active multi-line states
        match state {
            LineState::BlockComment => {
                let start = i;
                if let Some(rel) = line[i..].find("*/") {
                    i += rel + 2;
                    spans.push(Span { start, end: i, kind: TokenKind::Comment });
                    state = LineState::Normal;
                } else {
                    spans.push(Span { start, end: len, kind: TokenKind::Comment });
                    return (spans, LineState::BlockComment);
                }
                continue;
            }
            LineState::HtmlComment => {
                let start = i;
                if let Some(rel) = line[i..].find("-->") {
                    i += rel + 3;
                    spans.push(Span { start, end: i, kind: TokenKind::Comment });
                    state = LineState::Normal;
                } else {
                    spans.push(Span { start, end: len, kind: TokenKind::Comment });
                    return (spans, LineState::HtmlComment);
                }
                continue;
            }
            LineState::TripleDoubleStr => {
                let start = i;
                if let Some(rel) = line[i..].find("\"\"\"") {
                    i += rel + 3;
                    spans.push(Span { start, end: i, kind: TokenKind::String });
                    state = LineState::Normal;
                } else {
                    spans.push(Span { start, end: len, kind: TokenKind::String });
                    return (spans, LineState::TripleDoubleStr);
                }
                continue;
            }
            LineState::TripleSingleStr => {
                let start = i;
                if let Some(rel) = line[i..].find("'''") {
                    i += rel + 3;
                    spans.push(Span { start, end: i, kind: TokenKind::String });
                    state = LineState::Normal;
                } else {
                    spans.push(Span { start, end: len, kind: TokenKind::String });
                    return (spans, LineState::TripleSingleStr);
                }
                continue;
            }
            LineState::BacktickStr => {
                let (next_i, closed) = scan_quoted_with_escapes(line, i, b'`', TokenKind::String, &mut spans, false);
                i = next_i;
                if closed {
                    state = LineState::Normal;
                } else {
                    return (spans, LineState::BacktickStr);
                }
                continue;
            }
            LineState::Normal | LineState::MarkdownFence => {}
        }

        let b = bytes[i];
        let rest = &line[i..];

        // Skip whitespace
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Line comments
        if (matches!(
            lang,
            Language::Python | Language::Shell | Language::Yaml | Language::TomlIni | Language::Dockerfile
        ) && b == b'#')
            || (matches!(
                lang,
                Language::Rust | Language::JsTs | Language::CLike | Language::Go | Language::Json
            ) && rest.starts_with("//"))
            || (lang == Language::Sql && rest.starts_with("--"))
        {
            spans.push(Span { start: i, end: len, kind: TokenKind::Comment });
            break;
        }

        // Block comments
        if matches!(
            lang,
            Language::Rust | Language::JsTs | Language::CLike | Language::Go | Language::Sql | Language::HtmlCss
        ) && rest.starts_with("/*")
        {
            let start = i;
            i += 2;
            if let Some(rel) = line[i..].find("*/") {
                i += rel + 2;
                spans.push(Span { start, end: i, kind: TokenKind::Comment });
                state = LineState::Normal;
            } else {
                spans.push(Span { start, end: len, kind: TokenKind::Comment });
                return (spans, LineState::BlockComment);
            }
            continue;
        }

        if lang == Language::HtmlCss && rest.starts_with("<!--") {
            let start = i;
            i += 4;
            if let Some(rel) = line[i..].find("-->") {
                i += rel + 3;
                spans.push(Span { start, end: i, kind: TokenKind::Comment });
                state = LineState::Normal;
            } else {
                spans.push(Span { start, end: len, kind: TokenKind::Comment });
                return (spans, LineState::HtmlComment);
            }
            continue;
        }

        // HTML tag boundaries (`<div`, `</div>`, `>`, `/>`)
        if lang == Language::HtmlCss && b == b'<' {
            let start = i;
            i += 1;
            if i < len && (bytes[i] == b'/' || bytes[i] == b'!' || bytes[i] == b'?') {
                i += 1;
            }
            spans.push(Span { start, end: i, kind: TokenKind::Punctuation });
            in_html_tag = true;
            expect_html_tag_name = true;
            prev_non_ws_byte = Some(b'<');
            prev_word = "";
            continue;
        }
        if lang == Language::HtmlCss && in_html_tag && (b == b'>' || rest.starts_with("/>")) {
            let start = i;
            i += if rest.starts_with("/>") { 2 } else { 1 };
            spans.push(Span { start, end: i, kind: TokenKind::Punctuation });
            in_html_tag = false;
            expect_html_tag_name = false;
            prev_non_ws_byte = Some(b'>');
            prev_word = "";
            continue;
        }

        // Python triple-quoted strings
        if lang == Language::Python && (rest.starts_with("\"\"\"") || rest.starts_with("'''")) {
            let delim = if rest.starts_with("\"\"\"") { "\"\"\"" } else { "'''" };
            let target_state = if delim == "\"\"\"" {
                LineState::TripleDoubleStr
            } else {
                LineState::TripleSingleStr
            };
            let start = i;
            i += 3;
            if let Some(rel) = line[i..].find(delim) {
                i += rel + 3;
                spans.push(Span { start, end: i, kind: TokenKind::String });
            } else {
                spans.push(Span { start, end: len, kind: TokenKind::String });
                return (spans, target_state);
            }
            prev_non_ws_byte = Some(b'"');
            prev_word = "";
            continue;
        }

        // Backtick template / raw strings
        if matches!(lang, Language::JsTs | Language::Go | Language::Shell) && b == b'`' {
            let (next_i, closed) = scan_quoted_with_escapes(line, i, b'`', TokenKind::String, &mut spans, true);
            i = next_i;
            prev_non_ws_byte = Some(b'`');
            prev_word = "";
            if !closed && matches!(lang, Language::JsTs | Language::Go) {
                return (spans, LineState::BacktickStr);
            }
            continue;
        }

        // Double / Single quoted strings (with escape highlighting)
        if b == b'"' || (b == b'\'' && lang != Language::Rust) {
            let quote = b;
            // In JSON, `"key":` gets Key color (#87e6d8)
            let is_json_key = if lang == Language::Json && quote == b'"' {
                let end_q = find_closing_quote(bytes, i + 1, quote);
                line[end_q..].trim_start().starts_with(':')
            } else {
                false
            };
            let kind = if is_json_key { TokenKind::Key } else { TokenKind::String };
            let (next_i, _) = scan_quoted_with_escapes(line, i, quote, kind, &mut spans, true);
            i = next_i;
            prev_non_ws_byte = Some(quote);
            prev_word = "";
            continue;
        }

        // Rust char literal vs lifetime ('a, 'static, '\n', 'x')
        if lang == Language::Rust && b == b'\'' {
            let start = i;
            if i + 2 < len && bytes[i + 1] == b'\\' {
                let mut end = i + 2;
                while end < len && bytes[end] != b'\'' && end - i < 10 {
                    end += 1;
                }
                if end < len && bytes[end] == b'\'' {
                    spans.push(Span { start, end: start + 1, kind: TokenKind::String });
                    spans.push(Span { start: start + 1, end, kind: TokenKind::Escape });
                    spans.push(Span { start: end, end: end + 1, kind: TokenKind::String });
                    i = end + 1;
                    prev_non_ws_byte = Some(b'\'');
                    prev_word = "";
                    continue;
                }
            } else if i + 1 < len {
                // Single UTF-8 char literal `'a'`, `'▸'`
                let mut it = line[i + 1..].char_indices();
                if let Some((_, ch)) = it.next() {
                    let after = i + 1 + ch.len_utf8();
                    if after < len && bytes[after] == b'\'' {
                        spans.push(Span { start, end: after + 1, kind: TokenKind::String });
                        i = after + 1;
                        prev_non_ws_byte = Some(b'\'');
                        prev_word = "";
                        continue;
                    }
                }
            }
            // Lifetime `'a`, `'static`, `'_`
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            spans.push(Span { start, end: i, kind: TokenKind::Type });
            prev_non_ws_byte = Some(b'\'');
            prev_word = "";
            continue;
        }

        // C/C++ preprocessor directives (`#include <header>`, `#define NAME`, `#pragma ...`)
        if lang == Language::CLike && b == b'#' {
            let start = i;
            i += 1;
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let dir_start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let dir_word = &line[dir_start..i];
            spans.push(Span { start, end: i, kind: TokenKind::Keyword });
            // Highlight `<header.h>` as String right after `#include` (exact VS Code `string.quoted.other.lt-gt.include.cpp`)
            if dir_word == "include" {
                while i < len && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < len && bytes[i] == b'<' {
                    let hs = i;
                    if let Some(rel) = line[i..].find('>') {
                        i += rel + 1;
                        spans.push(Span { start: hs, end: i, kind: TokenKind::String });
                    }
                }
            }
            prev_word = dir_word;
            prev_non_ws_byte = Some(b'#');
            continue;
        }

        // Attributes / decorators (#[...], #![...], @decorator)
        if (lang == Language::Rust && (rest.starts_with("#[") || rest.starts_with("#![")))
            || (matches!(lang, Language::Python | Language::JsTs | Language::CLike) && b == b'@')
        {
            let start = i;
            if lang == Language::Rust {
                if let Some(rel) = rest.find(']') {
                    i += rel + 1;
                } else {
                    i = len;
                }
            } else {
                i += 1;
                while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.') {
                    i += 1;
                }
            }
            spans.push(Span { start, end: i, kind: TokenKind::Attribute });
            prev_non_ws_byte = Some(b'@');
            prev_word = "";
            continue;
        }

        // Shell / env variables ($VAR, ${VAR}, $1, $?)
        if matches!(lang, Language::Shell | Language::TomlIni | Language::Dockerfile) && b == b'$' {
            let start = i;
            i += 1;
            if i < len && bytes[i] == b'{' {
                if let Some(rel) = line[i..].find('}') {
                    i += rel + 1;
                } else {
                    i = len;
                }
            } else if i < len && matches!(bytes[i], b'?' | b'$' | b'#' | b'@' | b'*' | b'!') {
                i += 1;
            } else {
                while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
            }
            spans.push(Span { start, end: i, kind: TokenKind::Variable });
            prev_non_ws_byte = Some(b'$');
            prev_word = "";
            continue;
        }

        // Shell CLI flags (--flag, -la) — VS Code `constant.other.option.shell`
        if lang == Language::Shell
            && b == b'-'
            && (i == 0 || bytes[i - 1].is_ascii_whitespace())
            && i + 1 < len
            && (bytes[i + 1].is_ascii_alphanumeric() || bytes[i + 1] == b'-')
        {
            let start = i;
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_') {
                i += 1;
            }
            spans.push(Span { start, end: i, kind: TokenKind::Constant });
            prev_non_ws_byte = Some(b'-');
            prev_word = "";
            continue;
        }

        // Numbers (with VS Code sub-token highlighting for 0x/0b/0o prefixes, e/E/p/P exponents, +/- signs, suffixes)
        if b.is_ascii_digit() {
            i = scan_number(lang, line, i, &mut spans);
            prev_non_ws_byte = Some(b'0');
            prev_word = "";
            continue;
        }

        // Identifiers, Keywords, Constants, Types, Functions, Parameters, Properties
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            i += 1;
            while i < len
                && (bytes[i].is_ascii_alphanumeric()
                    || bytes[i] == b'_'
                    || (lang == Language::HtmlCss && bytes[i] == b'-'))
            {
                i += 1;
            }
            let word = &line[start..i];
            let rest_after = &line[i..];
            let next_non_ws = rest_after.bytes().find(|c| !c.is_ascii_whitespace());
            let next_is_single_colon = rest_after.starts_with(':') && !rest_after.starts_with("::");
            let after_dot = prev_non_ws_byte == Some(b'.');
            let is_param = param_ranges.contains(&(start, i));

            let kind = if lang == Language::HtmlCss {
                if expect_html_tag_name {
                    expect_html_tag_name = false;
                    TokenKind::Keyword
                } else if in_html_tag && next_non_ws == Some(b'=') {
                    TokenKind::Attribute
                } else if !in_html_tag && next_is_single_colon {
                    TokenKind::Property
                } else {
                    TokenKind::Plain
                }
            } else if is_param {
                TokenKind::Parameter
            } else if is_builtin_var(lang, word) {
                TokenKind::BuiltinVar
            } else if is_constant_literal(lang, word) {
                TokenKind::Constant
            } else if is_keyword(lang, word) && !after_dot {
                TokenKind::Keyword
            } else if matches!(prev_word, "fn" | "def" | "function" | "func") {
                TokenKind::Function
            } else if matches!(prev_word, "struct" | "enum" | "trait" | "class" | "interface" | "extends" | "implements") {
                TokenKind::Type
            } else if lang == Language::Rust && rest_after.starts_with('!') && !rest_after.starts_with("!=") {
                i += 1;
                TokenKind::Function
            } else if matches!(next_non_ws, Some(b'('))
                && !matches!(lang, Language::Json | Language::Yaml | Language::TomlIni)
            {
                if is_type_name(lang, word) && !after_dot {
                    TokenKind::Type
                } else {
                    TokenKind::Function
                }
            } else if lang == Language::Rust && rest_after.starts_with("::<") {
                TokenKind::Function
            } else if after_dot {
                TokenKind::Property
            } else if matches!(lang, Language::Rust | Language::Python) && is_screaming_const(word) {
                // Only Rust (`constant.other.caps.rust`) and Python (`constant.other.caps.python`)
                // highlight ALL_CAPS identifiers as constants in VS Code TextMate grammars;
                // C/C++/Java/C#/JS/TS/Go leave `const int INF = 10e9;` as plain variables.
                TokenKind::Constant
            } else if is_type_name(lang, word) {
                TokenKind::Type
            } else if is_builtin_func(lang, word) {
                TokenKind::Function
            } else if lang == Language::Shell && bytes.get(i) == Some(&b'=') {
                TokenKind::Variable
            } else if matches!(lang, Language::JsTs | Language::Rust)
                && next_is_single_colon
                && !matches!(prev_word, "let" | "const" | "var" | "mut" | "case" | "default" | "pub" | "static")
                && prev_non_ws_byte != Some(b'?')
            {
                TokenKind::Property
            } else {
                TokenKind::Plain
            };

            if kind != TokenKind::Plain {
                spans.push(Span { start, end: i, kind });
            }
            prev_word = word;
            prev_non_ws_byte = Some(b'a');
            continue;
        }

        // Punctuation & Braces (`() [] {} , ; .`) — VS Code #b8c0de
        if matches!(b, b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',' | b';' | b'.') {
            let start = i;
            i += 1;
            spans.push(Span { start, end: i, kind: TokenKind::Punctuation });
            prev_non_ws_byte = Some(b);
            if b != b'.' {
                prev_word = "";
            }
            continue;
        }

        // Operators (`= + - * / % ! < > & | ^ : ? ~`) — VS Code #ff73c6
        if matches!(
            b,
            b'=' | b'+' | b'-' | b'*' | b'/' | b'%' | b'!' | b'<' | b'>' | b'&' | b'|' | b'^' | b':' | b'?' | b'~'
        ) {
            let start = i;
            i += 1;
            // C/C++ arrow `->` makes next identifier a property!
            if lang == Language::CLike && b == b'-' && bytes.get(i) == Some(&b'>') {
                i += 1;
                spans.push(Span { start, end: i, kind: TokenKind::Operator });
                prev_non_ws_byte = Some(b'.');
                prev_word = "";
                continue;
            }
            spans.push(Span { start, end: i, kind: TokenKind::Operator });
            prev_non_ws_byte = Some(b);
            prev_word = "";
            continue;
        }

        // Advance one UTF-8 char
        let ch_len = line[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        i += ch_len;
        prev_word = "";
    }

    (spans, state)
}

fn scan_number(lang: Language, line: &str, mut i: usize, spans: &mut Vec<Span>) -> usize {
    let bytes = line.as_bytes();
    let len = bytes.len();

    // 1. Base prefix (`0x`, `0X`, `0b`, `0B`, `0o`, `0O`) -> VS Code `keyword.other.unit.*` / `storage.type.number`
    let mut is_hex = false;
    if bytes[i] == b'0'
        && i + 2 < len
        && matches!(bytes[i + 1], b'x' | b'X' | b'b' | b'B' | b'o' | b'O')
    {
        let p = bytes[i + 1];
        let next_valid = match p {
            b'x' | b'X' => bytes[i + 2].is_ascii_hexdigit() || bytes[i + 2] == b'_',
            b'b' | b'B' => matches!(bytes[i + 2], b'0' | b'1' | b'_'),
            _ => matches!(bytes[i + 2], b'0'..=b'7' | b'_'),
        };
        if next_valid {
            is_hex = matches!(p, b'x' | b'X');
            spans.push(Span { start: i, end: i + 2, kind: TokenKind::Keyword });
            i += 2;
        }
    }

    // 2. Mantissa digits (and optional single `.` decimal point, plus C++14 `'` digit separators)
    let mut seg_start = i;
    let mut saw_dot = false;
    while i < len {
        let c = bytes[i];
        let is_digit = if is_hex {
            c.is_ascii_hexdigit() || c == b'_'
        } else {
            c.is_ascii_digit() || c == b'_'
        };
        if is_digit {
            i += 1;
            continue;
        }
        if c == b'.' && !saw_dot && !(i + 1 < len && bytes[i + 1] == b'.') {
            // Only consume `.` if followed by a digit (don't eat method call `10.abs()`)
            let after_digit = i + 1 < len
                && if is_hex {
                    bytes[i + 1].is_ascii_hexdigit()
                } else {
                    bytes[i + 1].is_ascii_digit()
                };
            if after_digit {
                saw_dot = true;
                i += 1;
                continue;
            }
        }
        // C++14 digit separator: `1'000'000`
        if lang == Language::CLike
            && c == b'\''
            && i + 1 < len
            && if is_hex {
                bytes[i + 1].is_ascii_hexdigit()
            } else {
                bytes[i + 1].is_ascii_digit()
            }
        {
            if i > seg_start {
                spans.push(Span { start: seg_start, end: i, kind: TokenKind::Number });
            }
            spans.push(Span { start: i, end: i + 1, kind: TokenKind::Punctuation });
            i += 1;
            seg_start = i;
            continue;
        }
        break;
    }
    if i > seg_start {
        spans.push(Span { start: seg_start, end: i, kind: TokenKind::Number });
    }

    // 3. Scientific exponent (`e`/`E` for decimal, `p`/`P` for hex floats):
    //    `e`/`E` -> Keyword (`keyword.other.unit.exponent`), `+`/`-` -> Operator, digits -> Number
    if i < len
        && ((!is_hex && matches!(bytes[i], b'e' | b'E')) || (is_hex && matches!(bytes[i], b'p' | b'P')))
    {
        let mut exp_pos = i + 1;
        let has_sign = exp_pos < len && matches!(bytes[exp_pos], b'+' | b'-');
        if has_sign {
            exp_pos += 1;
        }
        if exp_pos < len && bytes[exp_pos].is_ascii_digit() {
            spans.push(Span { start: i, end: i + 1, kind: TokenKind::Keyword });
            i += 1;
            if has_sign {
                spans.push(Span { start: i, end: i + 1, kind: TokenKind::Operator });
                i += 1;
            }
            let es = i;
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
            spans.push(Span { start: es, end: i, kind: TokenKind::Number });
        }
    }

    // 4. Numeric type suffix (`LL`, `ull`, `f`, `u32`, `usize`, `f64`, `j`, `n`) -> Keyword (`storage.type.numeric`)
    if i < len && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
        let suf_start = i;
        while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        spans.push(Span { start: suf_start, end: i, kind: TokenKind::Keyword });
    }

    i
}

fn find_closing_quote(bytes: &[u8], mut i: usize, quote: u8) -> usize {
    let len = bytes.len();
    while i < len {
        if bytes[i] == b'\\' && i + 1 < len {
            i += 2;
            continue;
        }
        if bytes[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    len
}

fn scan_quoted_with_escapes(
    line: &str,
    mut i: usize,
    quote: u8,
    base_kind: TokenKind,
    spans: &mut Vec<Span>,
    include_open: bool,
) -> (usize, bool) {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut seg_start = i;
    if include_open && i < len {
        i += 1;
    }
    let mut closed = false;
    while i < len {
        if bytes[i] == b'\\' && i + 1 < len {
            if i > seg_start {
                spans.push(Span { start: seg_start, end: i, kind: base_kind });
            }
            let esc_start = i;
            let next = bytes[i + 1];
            i += 2;
            if next == b'x' && i + 2 <= len && bytes[i].is_ascii_hexdigit() && bytes[i + 1].is_ascii_hexdigit() {
                i += 2;
            } else if next == b'u' && i < len && bytes[i] == b'{' {
                if let Some(rel) = line[i..].find('}') {
                    i += rel + 1;
                }
            }
            spans.push(Span { start: esc_start, end: i, kind: TokenKind::Escape });
            seg_start = i;
            continue;
        }
        if bytes[i] == quote {
            i += 1;
            closed = true;
            break;
        }
        i += 1;
    }
    if i > seg_start {
        spans.push(Span { start: seg_start, end: i, kind: base_kind });
    }
    (i, closed)
}

/// Find parameter identifier byte ranges `(start, end)` in function/method signatures on a line.
fn find_param_ranges(lang: Language, line: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    match lang {
        Language::Rust => {
            if let Some((ps, pe)) = find_fn_paren_span(line, "fn ") {
                for (cs, ce) in split_top_level_commas(line, ps, pe) {
                    let chunk = &line[cs..ce];
                    if let Some(colon) = find_top_level_byte(chunk, b':') {
                        if let Some((is, ie)) = last_ident_in(chunk, 0, colon) {
                            let name = &chunk[is..ie];
                            if !matches!(name, "self" | "mut" | "ref") {
                                out.push((cs + is, cs + ie));
                            }
                        }
                    }
                }
            }
        }
        Language::Python => {
            if let Some((ps, pe)) = find_fn_paren_span(line, "def ") {
                for (cs, ce) in split_top_level_commas(line, ps, pe) {
                    let chunk = &line[cs..ce];
                    let limit = find_top_level_byte(chunk, b':')
                        .or_else(|| find_top_level_byte(chunk, b'='))
                        .unwrap_or(chunk.len());
                    if let Some((is, ie)) = first_ident_in(chunk, 0, limit) {
                        let name = &chunk[is..ie];
                        if !matches!(name, "self" | "cls") {
                            out.push((cs + is, cs + ie));
                        }
                    }
                }
            }
        }
        Language::JsTs => {
            let paren = find_fn_paren_span(line, "function ")
                .or_else(|| find_arrow_paren_span(line));
            if let Some((ps, pe)) = paren {
                for (cs, ce) in split_top_level_commas(line, ps, pe) {
                    let chunk = &line[cs..ce];
                    let limit = find_top_level_byte(chunk, b':')
                        .or_else(|| find_top_level_byte(chunk, b'?'))
                        .or_else(|| find_top_level_byte(chunk, b'='))
                        .unwrap_or(chunk.len());
                    if let Some((is, ie)) = first_ident_in(chunk, 0, limit) {
                        let name = &chunk[is..ie];
                        if !matches!(name, "this" | "readonly" | "public" | "private" | "protected") {
                            out.push((cs + is, cs + ie));
                        }
                    }
                }
            }
        }
        Language::Go => {
            if let Some((ps, pe)) = find_fn_paren_span(line, "func ") {
                for (cs, ce) in split_top_level_commas(line, ps, pe) {
                    let chunk = &line[cs..ce];
                    if let Some((is, ie)) = first_ident_in(chunk, 0, chunk.len()) {
                        let rest = chunk[ie..].trim_start();
                        if !rest.is_empty() {
                            out.push((cs + is, cs + ie));
                        }
                    }
                }
            }
        }
        Language::CLike => {
            let trimmed = line.trim_start();
            if !trimmed.starts_with("if")
                && !trimmed.starts_with("for")
                && !trimmed.starts_with("while")
                && !trimmed.starts_with("switch")
                && !trimmed.starts_with("catch")
                && !trimmed.starts_with("return")
            {
                if let Some(open) = line.find('(') {
                    if let Some(close) = find_matching_close_paren(line, open) {
                        let after = line[close + 1..].trim_start();
                        if after.starts_with('{') || after.starts_with("const") || after.starts_with("throws") {
                            for (cs, ce) in split_top_level_commas(line, open + 1, close) {
                                let chunk = &line[cs..ce];
                                let limit = find_top_level_byte(chunk, b'=').unwrap_or(chunk.len());
                                // Only mark if there are at least 2 identifiers/tokens (type + name)
                                if let Some((is, ie)) = last_ident_in(chunk, 0, limit) {
                                    if !chunk[..is].trim().is_empty() {
                                        out.push((cs + is, cs + ie));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
    out
}

fn find_fn_paren_span(line: &str, kw: &str) -> Option<(usize, usize)> {
    let idx = line.find(kw)?;
    if idx > 0 {
        let prev = line.as_bytes()[idx - 1];
        if prev.is_ascii_alphanumeric() || prev == b'_' {
            return None;
        }
    }
    let after = idx + kw.len();
    let bytes = line.as_bytes();
    let mut j = after;
    let mut angle = 0usize;
    while j < bytes.len() {
        match bytes[j] {
            b'<' => angle += 1,
            b'>' => angle = angle.saturating_sub(1),
            b'(' if angle == 0 => {
                let close = find_matching_close_paren(line, j)?;
                return Some((j + 1, close));
            }
            b'{' | b';' => return None,
            _ => {}
        }
        j += 1;
    }
    None
}

fn find_arrow_paren_span(line: &str) -> Option<(usize, usize)> {
    let arrow = line.find("=>")?;
    let before = line[..arrow].trim_end();
    if !before.ends_with(')') {
        return None;
    }
    let close = before.len() - 1;
    let bytes = before.as_bytes();
    let mut depth = 0i32;
    for j in (0..=close).rev() {
        if bytes[j] == b')' {
            depth += 1;
        } else if bytes[j] == b'(' {
            depth -= 1;
            if depth == 0 {
                return Some((j + 1, close));
            }
        }
    }
    None
}

fn find_matching_close_paren(line: &str, open: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut depth = 0i32;
    let mut in_q: Option<u8> = None;
    for (j, &b) in bytes.iter().enumerate().skip(open) {
        if let Some(q) = in_q {
            if b == q {
                in_q = None;
            }
            continue;
        }
        if b == b'"' || b == b'\'' {
            in_q = Some(b);
        } else if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth -= 1;
            if depth == 0 {
                return Some(j);
            }
        }
    }
    None
}

fn split_top_level_commas(line: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut parts = Vec::new();
    let bytes = line.as_bytes();
    let mut depth = 0i32;
    let mut cur = start;
    for j in start..end {
        match bytes[j] {
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            b')' | b']' | b'}' | b'>' => depth = (depth - 1).max(0),
            b',' if depth == 0 => {
                parts.push((cur, j));
                cur = j + 1;
            }
            _ => {}
        }
    }
    if cur < end {
        parts.push((cur, end));
    }
    parts
}

fn find_top_level_byte(s: &str, target: u8) -> Option<usize> {
    let mut depth = 0i32;
    for (j, &b) in s.as_bytes().iter().enumerate() {
        match b {
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            b')' | b']' | b'}' | b'>' => depth = (depth - 1).max(0),
            c if c == target && depth == 0 => return Some(j),
            _ => {}
        }
    }
    None
}

fn first_ident_in(s: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let bytes = s.as_bytes();
    let mut i = start;
    while i < end {
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let id_s = i;
            i += 1;
            while i < end && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            return Some((id_s, i));
        }
        i += 1;
    }
    None
}

fn last_ident_in(s: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let bytes = s.as_bytes();
    let mut last = None;
    let mut i = start;
    while i < end {
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let id_s = i;
            i += 1;
            while i < end && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            last = Some((id_s, i));
        } else {
            i += 1;
        }
    }
    last
}

fn find_yaml_colon(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut in_s = false;
    let mut in_d = false;
    for (idx, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_d => in_s = !in_s,
            b'"' if !in_s => in_d = !in_d,
            b'#' if !in_s && !in_d => return None,
            b':' if !in_s && !in_d => {
                if idx + 1 == bytes.len() || bytes[idx + 1].is_ascii_whitespace() {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

fn is_screaming_const(w: &str) -> bool {
    w.len() >= 2
        && w.bytes().any(|b| b.is_ascii_uppercase())
        && w.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

fn is_builtin_var(lang: Language, w: &str) -> bool {
    match lang {
        Language::Rust => matches!(w, "self" | "super"),
        Language::Python => matches!(w, "self" | "cls"),
        Language::JsTs | Language::CLike => matches!(w, "this" | "super" | "base"),
        _ => false,
    }
}

fn is_constant_literal(lang: Language, w: &str) -> bool {
    match lang {
        Language::Rust => matches!(w, "true" | "false"),
        Language::Python => matches!(w, "True" | "False" | "None"),
        Language::JsTs => matches!(w, "true" | "false" | "null" | "undefined" | "NaN" | "Infinity"),
        Language::Shell => matches!(w, "true" | "false"),
        Language::Json | Language::Yaml | Language::TomlIni => {
            matches!(w, "true" | "false" | "null" | "yes" | "no" | "on" | "off")
        }
        Language::Sql => {
            let up = w.to_ascii_uppercase();
            matches!(up.as_str(), "NULL" | "TRUE" | "FALSE")
        }
        Language::CLike => matches!(w, "true" | "false" | "null" | "nullptr" | "NULL"),
        Language::Go => matches!(w, "true" | "false" | "nil" | "iota"),
        _ => false,
    }
}

fn is_keyword(lang: Language, w: &str) -> bool {
    match lang {
        Language::Rust => matches!(
            w,
            // Keywords + Rust `storage.type.primitive.rust` / `storage.type.numeric.rust`
            "fn" | "let" | "mut" | "pub" | "struct" | "enum" | "impl" | "trait" | "for" | "in"
                | "while" | "loop" | "if" | "else" | "match" | "return" | "use" | "mod" | "crate"
                | "const" | "static" | "type" | "where" | "async" | "await" | "move" | "ref"
                | "unsafe" | "extern" | "as" | "break" | "continue" | "dyn"
                | "u8" | "u16" | "u32" | "u64" | "u128" | "usize"
                | "i8" | "i16" | "i32" | "i64" | "i128" | "isize"
                | "f16" | "f32" | "f64" | "f128" | "bool" | "char" | "str"
        ),
        Language::Python => matches!(
            w,
            "def" | "class" | "import" | "from" | "as" | "if" | "elif" | "else" | "for" | "while"
                | "in" | "not" | "and" | "or" | "is" | "return" | "yield" | "try" | "except"
                | "finally" | "raise" | "with" | "pass" | "break" | "continue" | "lambda"
                | "async" | "await" | "global" | "nonlocal" | "assert" | "del" | "match" | "case"
        ),
        Language::JsTs => matches!(
            w,
            "function" | "const" | "let" | "var" | "return" | "if" | "else" | "for" | "while"
                | "do" | "switch" | "case" | "default" | "break" | "continue" | "class"
                | "extends" | "implements" | "interface" | "type" | "enum" | "import" | "export"
                | "from" | "as" | "new" | "try" | "catch" | "finally" | "throw"
                | "async" | "await" | "typeof" | "instanceof" | "in" | "of" | "delete"
                | "readonly" | "public" | "private" | "protected" | "static" | "abstract"
                | "declare" | "namespace" | "keyof" | "infer" | "satisfies" | "constructor" | "override"
        ),
        Language::Shell => matches!(
            w,
            "if" | "then" | "else" | "elif" | "fi" | "for" | "in" | "do" | "done" | "while"
                | "until" | "case" | "esac" | "function" | "local" | "export" | "return" | "exit"
                | "source" | "alias" | "unalias" | "setopt" | "unsetopt" | "typeset" | "autoload"
                | "bindkey" | "zle" | "zstyle" | "sudo" | "param" | "foreach" | "select"
        ),
        Language::Dockerfile => matches!(
            w,
            "FROM" | "RUN" | "CMD" | "LABEL" | "EXPOSE" | "ENV" | "ADD" | "COPY" | "ENTRYPOINT"
                | "VOLUME" | "USER" | "WORKDIR" | "ARG" | "ONBUILD" | "HEALTHCHECK" | "SHELL" | "AS"
        ),
        Language::Sql => {
            let up = w.to_ascii_uppercase();
            matches!(
                up.as_str(),
                "SELECT" | "FROM" | "WHERE" | "JOIN" | "LEFT" | "RIGHT" | "INNER" | "OUTER" | "ON"
                    | "GROUP" | "BY" | "ORDER" | "LIMIT" | "OFFSET" | "INSERT" | "INTO" | "VALUES"
                    | "UPDATE" | "SET" | "DELETE" | "CREATE" | "TABLE" | "DROP" | "ALTER" | "INDEX"
                    | "AND" | "OR" | "NOT" | "IS" | "IN" | "AS" | "DISTINCT" | "HAVING"
                    | "UNION" | "CASE" | "WHEN" | "THEN" | "ELSE" | "END" | "PRIMARY" | "KEY"
                    | "FOREIGN" | "REFERENCES" | "EXISTS" | "LIKE" | "BETWEEN" | "ASC" | "DESC"
            )
        }
        Language::CLike => matches!(
            w,
            // Keywords + C/C++/Java/C# `storage.type.built-in.primitive` / `storage.type.primitive`
            "if" | "else" | "for" | "while" | "do" | "switch" | "case" | "default" | "break"
                | "continue" | "return" | "struct" | "class" | "enum" | "union" | "typedef"
                | "const" | "static" | "extern" | "public" | "private" | "protected" | "using"
                | "namespace" | "new" | "delete" | "var" | "sizeof" | "try" | "catch" | "throw"
                | "throws" | "finally" | "async" | "await" | "interface" | "extends" | "implements"
                | "package" | "import" | "final" | "override" | "virtual" | "inline" | "template"
                | "typename" | "auto" | "constexpr" | "consteval" | "noexcept" | "decltype"
                | "mutable" | "volatile" | "register" | "friend" | "operator" | "explicit"
                | "sealed" | "record" | "goto" | "delegate" | "event"
                | "void" | "bool" | "boolean" | "_Bool" | "byte" | "sbyte" | "char" | "wchar_t"
                | "char8_t" | "char16_t" | "char32_t" | "short" | "ushort" | "int" | "uint"
                | "long" | "ulong" | "float" | "double" | "decimal" | "signed" | "unsigned"
        ),
        Language::Go => matches!(
            w,
            // Keywords + Go `storage.type.*` built-in types
            "package" | "import" | "func" | "type" | "struct" | "interface" | "map" | "chan"
                | "var" | "const" | "if" | "else" | "for" | "range" | "switch" | "case" | "default"
                | "select" | "return" | "go" | "defer" | "break" | "continue" | "fallthrough" | "goto"
                | "string" | "bool" | "int" | "int8" | "int16" | "int32" | "int64"
                | "uint" | "uint8" | "uint16" | "uint32" | "uint64" | "uintptr"
                | "float32" | "float64" | "complex64" | "complex128" | "byte" | "rune" | "error"
        ),
        _ => false,
    }
}

fn is_builtin_func(lang: Language, w: &str) -> bool {
    match lang {
        Language::Python => matches!(
            w,
            "print" | "len" | "range" | "enumerate" | "zip" | "map" | "filter" | "sum" | "min"
                | "max" | "open" | "sorted" | "isinstance" | "issubclass" | "type" | "super"
                | "abs" | "all" | "any" | "chr" | "ord" | "repr" | "getattr" | "setattr"
                | "hasattr" | "id" | "input" | "iter" | "next" | "reversed" | "round" | "dir"
        ),
        Language::Shell => matches!(
            w,
            "echo" | "printf" | "cd" | "pwd" | "read" | "test" | "eval" | "exec" | "shift"
                | "unset" | "declare" | "readonly" | "trap" | "wait" | "kill" | "command"
                | "builtin" | "type" | "which" | "set" | "shopt" | "print" | "zmodload"
        ),
        Language::Go => matches!(
            w,
            "make" | "len" | "cap" | "append" | "copy" | "delete" | "close" | "new" | "panic" | "recover"
        ),
        _ => false,
    }
}

fn is_pascal_case(w: &str) -> bool {
    w.len() > 1
        && w.starts_with(|c: char| c.is_ascii_uppercase())
        && w.bytes().any(|b| b.is_ascii_lowercase())
}

fn is_type_name(lang: Language, w: &str) -> bool {
    match lang {
        Language::Rust => {
            matches!(
                w,
                "Self" | "String" | "Vec" | "Option" | "Result" | "Box" | "Arc" | "Rc" | "Mutex"
                    | "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet" | "Path" | "PathBuf"
                    | "Instant" | "Duration" | "Cow"
            ) || is_pascal_case(w)
        }
        Language::Python => {
            matches!(
                w,
                "int" | "float" | "str" | "bool" | "list" | "dict" | "tuple" | "set" | "bytes"
                    | "bytearray" | "complex" | "frozenset" | "object" | "Any" | "Optional"
                    | "Union" | "List" | "Dict" | "Tuple" | "Set" | "Callable" | "Path"
                    | "Sequence" | "Mapping" | "Iterable"
            ) || is_pascal_case(w)
        }
        Language::JsTs => {
            matches!(
                w,
                "string" | "number" | "boolean" | "any" | "unknown" | "never" | "void" | "object"
                    | "symbol" | "bigint" | "console" | "Math" | "JSON" | "Promise" | "Error"
                    | "Array" | "Object" | "Map" | "Set" | "Date" | "RegExp" | "Number" | "String"
                    | "Boolean" | "URL" | "Record" | "Partial" | "Required" | "Readonly" | "Pick" | "Omit"
            ) || is_pascal_case(w)
        }
        Language::CLike => {
            matches!(
                w,
                "size_t" | "ssize_t" | "ptrdiff_t" | "intptr_t" | "uintptr_t" | "time_t"
                    | "int8_t" | "int16_t" | "int32_t" | "int64_t"
                    | "uint8_t" | "uint16_t" | "uint32_t" | "uint64_t"
                    | "string" | "string_view" | "vector" | "map" | "unordered_map" | "set"
                    | "unordered_set" | "multiset" | "multimap" | "pair" | "tuple" | "array"
                    | "queue" | "stack" | "priority_queue" | "deque" | "bitset" | "list"
                    | "shared_ptr" | "unique_ptr" | "weak_ptr" | "optional" | "variant"
                    | "stringstream" | "ifstream" | "ofstream" | "istream" | "ostream"
            ) || is_pascal_case(w)
        }
        Language::Go => matches!(w, "any" | "comparable") || is_pascal_case(w),
        _ => false,
    }
}

/// Find matching bracket byte offset for `() [] {}`.
/// - In thin bar cursor mode (`at_cursor = false`): only triggers when the cursor is
///   immediately AFTER a delimiter (`byte_pos - 1`), never before `([{` or `)]}`.
/// - In Vim block cursor mode (`at_cursor = true`): triggers on the delimiter directly
///   under the block cursor (`byte_pos`).
pub fn find_matching_bracket(rope: &Rope, byte_pos: usize, at_cursor: bool) -> Option<(usize, usize)> {
    let len = rope.len_bytes();
    if len == 0 {
        return None;
    }

    let pos = if at_cursor {
        if byte_pos < len { byte_pos } else { return None }
    } else if byte_pos > 0 {
        byte_pos - 1
    } else {
        return None;
    };

    for pos in [pos] {
        let s = rope.slice(pos, (pos + 1).min(len));
        let b = match s.as_bytes().first().copied() {
            Some(c) => c,
            None => continue,
        };
        let (open, close, forward) = match b {
            b'(' => (b'(', b')', true),
            b'[' => (b'[', b']', true),
            b'{' => (b'{', b'}', true),
            b')' => (b'(', b')', false),
            b']' => (b'[', b']', false),
            b'}' => (b'{', b'}', false),
            _ => continue,
        };

        // Bounded scan up to 32 KB in either direction for guaranteed <1ms responsiveness
        const MAX_SCAN: usize = 32_768;
        if forward {
            let end = (pos + MAX_SCAN).min(len);
            let chunk = rope.slice(pos, end);
            let mut depth = 0i32;
            let mut in_quote: Option<u8> = None;
            let mut escaped = false;
            for (i, &c) in chunk.as_bytes().iter().enumerate() {
                if let Some(q) = in_quote {
                    if escaped {
                        escaped = false;
                    } else if c == b'\\' {
                        escaped = true;
                    } else if c == q {
                        in_quote = None;
                    }
                    continue;
                }
                if i > 0 && (c == b'"' || c == b'\'') {
                    in_quote = Some(c);
                    continue;
                }
                if c == open {
                    depth += 1;
                } else if c == close {
                    depth -= 1;
                    if depth == 0 {
                        return Some((pos, pos + i));
                    }
                }
            }
        } else {
            let start = pos.saturating_sub(MAX_SCAN);
            let chunk = rope.slice(start, pos + 1);
            let bytes = chunk.as_bytes();
            let mut depth = 0i32;
            for i in (0..bytes.len()).rev() {
                let c = bytes[i];
                if c == close {
                    depth += 1;
                } else if c == open {
                    depth -= 1;
                    if depth == 0 {
                        return Some((pos, start + i));
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_block_comment_and_python_docstring() {
        let r = Rope::from_str("let x = 1; /* start\nmiddle line\nend */ let y = 2;");
        let mut cache = SyntaxCache::new(Language::Rust);
        let s0 = cache.line_spans(&r, 0);
        let s1 = cache.line_spans(&r, 1);
        let s2 = cache.line_spans(&r, 2);
        assert!(s0.iter().any(|sp| sp.kind == TokenKind::Comment));
        assert_eq!(s1.len(), 1);
        assert_eq!(s1[0].kind, TokenKind::Comment);
        assert!(s2.iter().any(|sp| sp.kind == TokenKind::Keyword));

        let py = Rope::from_str("\"\"\"doc\nstill doc\n\"\"\"\ndef f(): pass");
        let mut pcache = SyntaxCache::new(Language::Python);
        let ps1 = pcache.line_spans(&py, 1);
        assert_eq!(ps1[0].kind, TokenKind::String);
        let ps3 = pcache.line_spans(&py, 3);
        assert!(ps3.iter().any(|sp| sp.kind == TokenKind::Keyword));
    }

    #[test]
    fn bracket_matching_nested() {
        let r = Rope::from_str("fn f(a: Vec<(i32, i32)>) { let s = \")\"; }");
        // Bar cursor (`at_cursor = false`): NEVER before delimiter, ONLY immediately after!
        assert_eq!(find_matching_bracket(&r, 4, false), None);           // before '('
        assert_eq!(find_matching_bracket(&r, 5, false), Some((4, 23)));  // after '('
        assert_eq!(find_matching_bracket(&r, 23, false), None);          // before ')'
        assert_eq!(find_matching_bracket(&r, 24, false), Some((23, 4))); // after ')'
        assert_eq!(find_matching_bracket(&r, 25, false), None);          // before '{'
        assert_eq!(find_matching_bracket(&r, 26, false), Some((25, 40))); // after '{'
        // Block cursor (`at_cursor = true`): directly under block
        assert_eq!(find_matching_bracket(&r, 4, true), Some((4, 23)));
        assert_eq!(find_matching_bracket(&r, 23, true), Some((23, 4)));
        assert_eq!(find_matching_bracket(&r, 25, true), Some((25, 40)));
    }

    #[test]
    fn vscode_dracula_token_scopes() {
        // Rust: fn, name, param, type, self, property, method, macro, constant, string + escape, punctuation
        let rust_line = "pub fn greet(&self, name: String) { println!(\"hi\\n\", self.cfg, MAX_LEN, true); }";
        let (spans, _) = lex_line(Language::Rust, rust_line, LineState::Normal);
        let kind_of = |needle: &str| {
            let s = rust_line.find(needle).unwrap();
            let e = s + needle.len();
            spans.iter().find(|sp| sp.start == s && sp.end == e).map(|sp| sp.kind)
        };
        assert_eq!(kind_of("pub"), Some(TokenKind::Keyword));
        assert_eq!(kind_of("fn"), Some(TokenKind::Keyword));
        assert_eq!(kind_of("greet"), Some(TokenKind::Function));
        assert_eq!(kind_of("self"), Some(TokenKind::BuiltinVar));
        assert_eq!(kind_of("name"), Some(TokenKind::Parameter));
        assert_eq!(kind_of("String"), Some(TokenKind::Type));
        assert_eq!(kind_of("println!"), Some(TokenKind::Function));
        assert_eq!(kind_of("\\n"), Some(TokenKind::Escape));
        assert_eq!(kind_of("cfg"), Some(TokenKind::Property));
        assert_eq!(kind_of("MAX_LEN"), Some(TokenKind::Constant));
        assert_eq!(kind_of("true"), Some(TokenKind::Constant));

        // Python: def, param, self, None, comment
        let py_line = "def compute(self, count: int) -> None: # calc";
        let (pspans, _) = lex_line(Language::Python, py_line, LineState::Normal);
        let pkind = |needle: &str| {
            let s = py_line.find(needle).unwrap();
            let e = s + needle.len();
            pspans.iter().find(|sp| sp.start == s && sp.end == e).map(|sp| sp.kind)
        };
        assert_eq!(pkind("def"), Some(TokenKind::Keyword));
        assert_eq!(pkind("compute"), Some(TokenKind::Function));
        assert_eq!(pkind("self"), Some(TokenKind::BuiltinVar));
        assert_eq!(pkind("count"), Some(TokenKind::Parameter));
        assert_eq!(pkind("int"), Some(TokenKind::Type));
        assert_eq!(pkind("None"), Some(TokenKind::Constant));
        assert_eq!(pkind("# calc"), Some(TokenKind::Comment));

        // C++: exact VS Code TextMate alignment (`const int INF = 10e9;` + `#include <bits/stdc++.h>`)
        let cpp_line = "const int INF = 10e9;";
        let (cspans, _) = lex_line(Language::CLike, cpp_line, LineState::Normal);
        let ckind = |needle: &str| {
            let s = cpp_line.find(needle).unwrap();
            let e = s + needle.len();
            cspans.iter().find(|sp| sp.start == s && sp.end == e).map(|sp| sp.kind)
        };
        assert_eq!(ckind("const"), Some(TokenKind::Keyword));
        assert_eq!(ckind("int"), Some(TokenKind::Keyword));
        assert_eq!(ckind("INF"), None); // Plain white variable, matching VS Code!
        assert_eq!(ckind("="), Some(TokenKind::Operator));
        assert_eq!(ckind("10"), Some(TokenKind::Number));
        assert_eq!(ckind("e"), Some(TokenKind::Keyword));
        assert_eq!(ckind("9"), Some(TokenKind::Number));
        assert_eq!(ckind(";"), Some(TokenKind::Punctuation));

        // Verify hex literals do NOT split on hex digit 'e', while scientific signs & suffixes DO split
        let num_line = "0x1e9f 1e-5 42usize 100LL 1'000";
        let (nspans, _) = lex_line(Language::CLike, num_line, LineState::Normal);
        let nkind = |s: usize, e: usize| nspans.iter().find(|sp| sp.start == s && sp.end == e).map(|sp| sp.kind);
        assert_eq!(nkind(0, 2), Some(TokenKind::Keyword)); // 0x
        assert_eq!(nkind(2, 6), Some(TokenKind::Number)); // 1e9f (whole hex!)
        assert_eq!(nkind(7, 8), Some(TokenKind::Number)); // 1
        assert_eq!(nkind(8, 9), Some(TokenKind::Keyword)); // e
        assert_eq!(nkind(9, 10), Some(TokenKind::Operator)); // -
        assert_eq!(nkind(10, 11), Some(TokenKind::Number)); // 5
        assert_eq!(nkind(12, 14), Some(TokenKind::Number)); // 42
        assert_eq!(nkind(14, 19), Some(TokenKind::Keyword)); // usize
        assert_eq!(nkind(20, 23), Some(TokenKind::Number)); // 100
        assert_eq!(nkind(23, 25), Some(TokenKind::Keyword)); // LL
        assert_eq!(nkind(26, 27), Some(TokenKind::Number)); // 1
        assert_eq!(nkind(27, 28), Some(TokenKind::Punctuation)); // '
        assert_eq!(nkind(28, 31), Some(TokenKind::Number)); // 000

        let inc_line = "#include <bits/stdc++.h>";
        let (ispans, _) = lex_line(Language::CLike, inc_line, LineState::Normal);
        assert_eq!(ispans[0].kind, TokenKind::Keyword);
        assert_eq!(ispans[1].kind, TokenKind::String);
    }
}
