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
    BlockComment,     // /* ... */
    HtmlComment,      // <!-- ... -->
    TripleDoubleStr,  // """ ... """
    TripleSingleStr,  // ''' ... '''
    BacktickStr,      // ` ... `
    MarkdownFence,    // ``` ... ```
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
            spans.push(Span { start: 0, end: len, kind: TokenKind::Attribute });
            let next = if state == LineState::MarkdownFence {
                LineState::Normal
            } else {
                LineState::MarkdownFence
            };
            return (spans, next);
        }
        if state == LineState::MarkdownFence {
            spans.push(Span { start: 0, end: len, kind: TokenKind::String });
            return (spans, state);
        }
        if trimmed.starts_with('#') {
            spans.push(Span { start: 0, end: len, kind: TokenKind::Heading });
            return (spans, state);
        }
        if trimmed.starts_with('>') {
            spans.push(Span { start: 0, end: len, kind: TokenKind::Comment });
            return (spans, state);
        }
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
            spans.push(Span { start: 0, end: eq, kind: TokenKind::Key });
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
            let kstart = if trimmed.starts_with("- ") { lead + 2 } else { lead };
            if kstart < colon {
                spans.push(Span { start: kstart, end: colon, kind: TokenKind::Key });
            }
            spans.push(Span { start: colon, end: colon + 1, kind: TokenKind::Operator });
            i = colon + 1;
        }
    }

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
                let start = i;
                let mut closed = false;
                while i < len {
                    if bytes[i] == b'\\' && i + 1 < len {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'`' {
                        i += 1;
                        closed = true;
                        break;
                    }
                    i += 1;
                }
                spans.push(Span { start, end: i, kind: TokenKind::String });
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
            continue;
        }

        // Backtick template / raw strings
        if matches!(lang, Language::JsTs | Language::Go | Language::Markdown | Language::Shell) && b == b'`' {
            let start = i;
            i += 1;
            let mut closed = false;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'`' {
                    i += 1;
                    closed = true;
                    break;
                }
                i += 1;
            }
            spans.push(Span { start, end: i, kind: TokenKind::String });
            if !closed && matches!(lang, Language::JsTs | Language::Go) {
                return (spans, LineState::BacktickStr);
            }
            continue;
        }

        // Double / Single quoted strings
        if b == b'"' || (b == b'\'' && lang != Language::Rust) {
            let quote = b;
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            // In JSON, `"key":` gets Key color
            let kind = if lang == Language::Json && line[i..].trim_start().starts_with(':') {
                TokenKind::Key
            } else {
                TokenKind::String
            };
            spans.push(Span { start, end: i, kind });
            continue;
        }

        // Rust char literal vs lifetime
        if lang == Language::Rust && b == b'\'' {
            let start = i;
            if i + 2 < len && bytes[i + 1] == b'\\' && i + 3 < len && bytes[i + 3] == b'\'' {
                i += 4;
                spans.push(Span { start, end: i, kind: TokenKind::String });
                continue;
            } else if i + 2 < len && bytes[i + 2] == b'\'' {
                i += 3;
                spans.push(Span { start, end: i, kind: TokenKind::String });
                continue;
            } else {
                i += 1;
                while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                spans.push(Span { start, end: i, kind: TokenKind::Type });
                continue;
            }
        }

        // Attributes / decorators (#[...], @decorator)
        if (lang == Language::Rust && rest.starts_with("#["))
            || (lang == Language::Python && b == b'@')
            || (lang == Language::JsTs && b == b'@')
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
            continue;
        }

        // Shell / PHP / env variables ($VAR, ${VAR})
        if matches!(lang, Language::Shell | Language::TomlIni | Language::Dockerfile) && b == b'$' {
            let start = i;
            i += 1;
            if i < len && bytes[i] == b'{' {
                if let Some(rel) = line[i..].find('}') {
                    i += rel + 1;
                } else {
                    i = len;
                }
            } else {
                while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
            }
            spans.push(Span { start, end: i, kind: TokenKind::Variable });
            continue;
        }

        // Numbers
        if b.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < len
                && (bytes[i].is_ascii_alphanumeric()
                    || bytes[i] == b'_'
                    || bytes[i] == b'.'
                    || bytes[i] == b'x'
                    || bytes[i] == b'X')
            {
                i += 1;
            }
            spans.push(Span { start, end: i, kind: TokenKind::Number });
            continue;
        }

        // Identifiers & Keywords
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];
            let next_non_ws = line[i..].bytes().find(|c| !c.is_ascii_whitespace());

            let kind = if is_keyword(lang, word) {
                TokenKind::Keyword
            } else if is_type_name(lang, word) {
                TokenKind::Type
            } else if matches!(next_non_ws, Some(b'('))
                && !matches!(lang, Language::Json | Language::Yaml | Language::TomlIni)
            {
                TokenKind::Function
            } else if lang == Language::Rust && next_non_ws == Some(b'!') {
                i += 1;
                TokenKind::Attribute
            } else if lang == Language::Shell && bytes.get(i) == Some(&b'=') {
                TokenKind::Variable
            } else {
                TokenKind::Plain
            };

            if kind != TokenKind::Plain {
                spans.push(Span { start, end: i, kind });
            }
            continue;
        }

        // Operators
        if matches!(b, b'=' | b'+' | b'-' | b'*' | b'/' | b'%' | b'!' | b'<' | b'>' | b'&' | b'|' | b'^' | b':' | b'?') {
            let start = i;
            i += 1;
            spans.push(Span { start, end: i, kind: TokenKind::Operator });
            continue;
        }

        // Advance one UTF-8 char
        let ch_len = line[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        i += ch_len;
    }

    (spans, state)
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

fn is_keyword(lang: Language, w: &str) -> bool {
    match lang {
        Language::Rust => matches!(
            w,
            "fn" | "let" | "mut" | "pub" | "struct" | "enum" | "impl" | "trait" | "for" | "in"
                | "while" | "loop" | "if" | "else" | "match" | "return" | "use" | "mod" | "crate"
                | "const" | "static" | "type" | "where" | "async" | "await" | "move" | "ref"
                | "unsafe" | "extern" | "as" | "break" | "continue" | "true" | "false" | "self"
                | "Self" | "super"
        ),
        Language::Python => matches!(
            w,
            "def" | "class" | "import" | "from" | "as" | "if" | "elif" | "else" | "for" | "while"
                | "in" | "not" | "and" | "or" | "is" | "return" | "yield" | "try" | "except"
                | "finally" | "raise" | "with" | "pass" | "break" | "continue" | "lambda"
                | "async" | "await" | "global" | "nonlocal" | "True" | "False" | "None" | "assert"
        ),
        Language::JsTs => matches!(
            w,
            "function" | "const" | "let" | "var" | "return" | "if" | "else" | "for" | "while"
                | "do" | "switch" | "case" | "default" | "break" | "continue" | "class"
                | "extends" | "implements" | "interface" | "type" | "enum" | "import" | "export"
                | "from" | "as" | "new" | "this" | "super" | "try" | "catch" | "finally" | "throw"
                | "async" | "await" | "typeof" | "instanceof" | "in" | "of" | "true" | "false"
                | "null" | "undefined" | "void" | "readonly" | "public" | "private"
        ),
        Language::Shell => matches!(
            w,
            "if" | "then" | "else" | "elif" | "fi" | "for" | "in" | "do" | "done" | "while"
                | "until" | "case" | "esac" | "function" | "local" | "export" | "return" | "exit"
                | "source" | "alias" | "unalias" | "setopt" | "unsetopt" | "typeset" | "autoload"
                | "bindkey" | "zle" | "zstyle" | "true" | "false" | "sudo" | "param" | "foreach"
        ),
        Language::Json | Language::Yaml | Language::TomlIni => {
            matches!(w, "true" | "false" | "null" | "yes" | "no" | "on" | "off")
        }
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
                    | "AND" | "OR" | "NOT" | "NULL" | "IS" | "IN" | "AS" | "DISTINCT" | "HAVING"
                    | "UNION" | "CASE" | "WHEN" | "THEN" | "ELSE" | "END" | "PRIMARY" | "KEY"
            )
        }
        Language::CLike => matches!(
            w,
            "if" | "else" | "for" | "while" | "do" | "switch" | "case" | "default" | "break"
                | "continue" | "return" | "struct" | "class" | "enum" | "union" | "typedef"
                | "const" | "static" | "extern" | "public" | "private" | "protected" | "using"
                | "namespace" | "new" | "delete" | "var" | "void" | "true" | "false" | "null"
                | "nullptr" | "sizeof" | "try" | "catch" | "throw" | "async" | "await"
        ),
        Language::Go => matches!(
            w,
            "package" | "import" | "func" | "type" | "struct" | "interface" | "map" | "chan"
                | "var" | "const" | "if" | "else" | "for" | "range" | "switch" | "case" | "default"
                | "select" | "return" | "go" | "defer" | "break" | "continue" | "true" | "false"
                | "nil" | "make" | "len" | "append"
        ),
        _ => false,
    }
}

fn is_type_name(lang: Language, w: &str) -> bool {
    match lang {
        Language::Rust => {
            matches!(
                w,
                "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64"
                    | "i128" | "isize" | "f32" | "f64" | "bool" | "char" | "str" | "String"
                    | "Vec" | "Option" | "Result" | "Box" | "Arc" | "Mutex" | "HashMap" | "Path"
                    | "PathBuf"
            ) || w.starts_with(|c: char| c.is_ascii_uppercase())
        }
        Language::Python => matches!(
            w,
            "int" | "float" | "str" | "bool" | "list" | "dict" | "tuple" | "set" | "bytes"
                | "Any" | "Optional" | "List" | "Dict" | "Path"
        ),
        Language::JsTs | Language::CLike | Language::Go => {
            matches!(
                w,
                "string" | "number" | "boolean" | "int" | "uint" | "float32" | "float64" | "byte"
                    | "rune" | "error" | "double" | "long" | "short" | "char" | "size_t" | "uint32_t"
            ) || (w.len() > 1 && w.starts_with(|c: char| c.is_ascii_uppercase()) && !w.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
        }
        _ => false,
    }
}

/// Find matching bracket byte offset for `() [] {}` at or immediately before `byte_pos`.
pub fn find_matching_bracket(rope: &Rope, byte_pos: usize) -> Option<(usize, usize)> {
    let len = rope.len_bytes();
    if len == 0 {
        return None;
    }

    // Check char at cursor, then char immediately before cursor
    let check_positions = [
        (byte_pos < len).then_some(byte_pos),
        (byte_pos > 0).then_some(byte_pos.saturating_sub(1)),
    ];

    for pos in check_positions.into_iter().flatten() {
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
        assert_eq!(find_matching_bracket(&r, 4), Some((4, 23)));
        assert_eq!(find_matching_bracket(&r, 23), Some((23, 4)));
        assert_eq!(find_matching_bracket(&r, 25), Some((25, 40)));
    }
}
