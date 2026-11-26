use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

pub type Rgb = (u8, u8, u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Plain,
    Keyword,
    Type,
    Function,
    String,
    Escape,
    Number,
    Constant,
    BuiltinVar,
    Parameter,
    Property,
    Comment,
    Operator,
    Punctuation,
    Variable,
    Attribute,
    Key,
    Heading,
}

#[derive(Clone, Copy, Debug)]
pub struct TokenStyle {
    pub fg: Rgb,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub name: &'static str,
    pub cursor_hex: &'static str,
    pub grad: [Rgb; 3],
    pub fg: Rgb,
    pub dim: Rgb,
    pub line_num: Rgb,
    pub active_line_num: Rgb,
    pub border: Rgb,
    pub indent_guide: Rgb,
    pub indent_active: Rgb,
    pub cur_line_bg: Rgb,
    pub sel_bg: Rgb,
    pub word_hl_bg: Rgb,
    pub match_bg: Rgb,
    pub active_match_bg: Rgb,
    pub bracket_bg: Rgb,
    pub bracket_fg: Rgb,
    pub bar_bg: Rgb,
    pub tab_active_bg: Rgb,
    pub ok: Rgb,
    pub warn: Rgb,
    pub err: Rgb,
    pub kw: Rgb,
    pub kw_bold: bool,
    pub ty: Rgb,
    pub func: Rgb,
    pub str_col: Rgb,
    pub escape: Rgb,
    pub num: Rgb,
    pub constant: Rgb,
    pub param: Rgb,
    pub prop: Rgb,
    pub comment: Rgb,
    pub op: Rgb,
    pub punct: Rgb,
    pub var: Rgb,
    pub attr: Rgb,
    pub key: Rgb,
    pub heading: Rgb,
}

pub const PALETTES: [Palette; 6] = [
    Palette {
        name: "cyber",
        cursor_hex: "#00f0ff",
        grad: [(0, 245, 255), (124, 77, 255), (255, 46, 151)],
        fg: (241, 245, 249),
        dim: (100, 108, 142),
        line_num: (100, 108, 142),
        active_line_num: (0, 245, 255),
        border: (59, 47, 99),
        indent_guide: (59, 47, 99),
        indent_active: (124, 77, 255),
        cur_line_bg: (20, 23, 38),
        sel_bg: (42, 54, 96),
        word_hl_bg: (38, 44, 74),
        match_bg: (56, 48, 84),
        active_match_bg: (124, 77, 255),
        bracket_bg: (28, 66, 86),
        bracket_fg: (0, 245, 255),
        bar_bg: (16, 18, 30),
        tab_active_bg: (28, 32, 54),
        ok: (0, 255, 136),
        warn: (255, 196, 0),
        err: (255, 30, 102),
        kw: (192, 132, 252),
        kw_bold: true,
        ty: (0, 245, 255),
        func: (86, 204, 255),
        str_col: (255, 214, 0),
        escape: (255, 46, 151),
        num: (255, 118, 178),
        constant: (255, 118, 178),
        param: (255, 181, 122),
        prop: (125, 249, 255),
        comment: (106, 114, 148),
        op: (255, 46, 151),
        punct: (168, 178, 210),
        var: (125, 249, 255),
        attr: (0, 255, 136),
        key: (0, 240, 255),
        heading: (0, 245, 255),
    },
    Palette {
        name: "emerald",
        cursor_hex: "#00ff88",
        grad: [(0, 255, 136), (0, 229, 255), (178, 255, 89)],
        fg: (236, 253, 245),
        dim: (78, 126, 98),
        line_num: (78, 126, 98),
        active_line_num: (0, 255, 136),
        border: (31, 89, 59),
        indent_guide: (31, 89, 59),
        indent_active: (0, 229, 255),
        cur_line_bg: (14, 29, 20),
        sel_bg: (26, 72, 48),
        word_hl_bg: (24, 54, 42),
        match_bg: (30, 66, 54),
        active_match_bg: (0, 140, 96),
        bracket_bg: (24, 72, 58),
        bracket_fg: (0, 255, 136),
        bar_bg: (11, 22, 16),
        tab_active_bg: (20, 44, 30),
        ok: (0, 255, 136),
        warn: (255, 234, 0),
        err: (255, 30, 102),
        kw: (0, 255, 136),
        kw_bold: true,
        ty: (0, 229, 255),
        func: (120, 255, 180),
        str_col: (255, 234, 0),
        escape: (0, 229, 255),
        num: (178, 255, 89),
        constant: (178, 255, 89),
        param: (255, 204, 112),
        prop: (134, 239, 212),
        comment: (84, 130, 104),
        op: (0, 229, 255),
        punct: (162, 206, 184),
        var: (167, 243, 208),
        attr: (178, 255, 89),
        key: (0, 245, 212),
        heading: (0, 255, 136),
    },
    Palette {
        name: "sunset",
        cursor_hex: "#ff5e3a",
        grad: [(255, 214, 0), (255, 94, 58), (255, 30, 142)],
        fg: (255, 241, 242),
        dim: (140, 84, 112),
        line_num: (140, 84, 112),
        active_line_num: (255, 214, 0),
        border: (92, 37, 64),
        indent_guide: (92, 37, 64),
        indent_active: (255, 94, 58),
        cur_line_bg: (30, 16, 24),
        sel_bg: (82, 32, 56),
        word_hl_bg: (60, 32, 42),
        match_bg: (76, 42, 34),
        active_match_bg: (190, 52, 88),
        bracket_bg: (86, 44, 42),
        bracket_fg: (255, 214, 0),
        bar_bg: (22, 12, 18),
        tab_active_bg: (48, 22, 36),
        ok: (0, 255, 136),
        warn: (255, 214, 0),
        err: (255, 23, 68),
        kw: (255, 46, 151),
        kw_bold: true,
        ty: (255, 159, 28),
        func: (255, 118, 82),
        str_col: (255, 214, 0),
        escape: (255, 46, 151),
        num: (255, 94, 178),
        constant: (255, 94, 178),
        param: (255, 198, 128),
        prop: (255, 160, 112),
        comment: (146, 92, 118),
        op: (255, 94, 58),
        punct: (212, 172, 184),
        var: (255, 198, 128),
        attr: (255, 214, 0),
        key: (255, 140, 90),
        heading: (255, 214, 0),
    },
    Palette {
        name: "nord",
        cursor_hex: "#38e8ff",
        grad: [(56, 232, 255), (108, 140, 255), (192, 132, 252)],
        fg: (241, 245, 249),
        dim: (92, 112, 150),
        line_num: (92, 112, 150),
        active_line_num: (56, 232, 255),
        border: (44, 63, 102),
        indent_guide: (44, 63, 102),
        indent_active: (108, 140, 255),
        cur_line_bg: (18, 26, 43),
        sel_bg: (42, 64, 108),
        word_hl_bg: (34, 48, 76),
        match_bg: (46, 58, 92),
        active_match_bg: (90, 116, 210),
        bracket_bg: (32, 58, 88),
        bracket_fg: (56, 232, 255),
        bar_bg: (13, 19, 32),
        tab_active_bg: (26, 38, 64),
        ok: (52, 255, 180),
        warn: (255, 215, 95),
        err: (255, 95, 115),
        kw: (192, 132, 252),
        kw_bold: true,
        ty: (56, 232, 255),
        func: (108, 168, 255),
        str_col: (255, 215, 95),
        escape: (192, 132, 252),
        num: (165, 180, 252),
        constant: (165, 180, 252),
        param: (255, 186, 134),
        prop: (136, 224, 216),
        comment: (96, 116, 154),
        op: (108, 140, 255),
        punct: (166, 182, 212),
        var: (147, 235, 255),
        attr: (52, 255, 180),
        key: (56, 232, 255),
        heading: (56, 232, 255),
    },
    Palette {
        name: "stealth",
        cursor_hex: "#a855f7",
        grad: [(248, 250, 252), (168, 85, 247), (236, 72, 153)],
        fg: (244, 244, 248),
        dim: (112, 98, 138),
        line_num: (112, 98, 138),
        active_line_num: (216, 180, 254),
        border: (76, 40, 117),
        indent_guide: (76, 40, 117),
        indent_active: (168, 85, 247),
        cur_line_bg: (21, 18, 31),
        sel_bg: (64, 38, 98),
        word_hl_bg: (46, 32, 68),
        match_bg: (58, 40, 82),
        active_match_bg: (148, 68, 224),
        bracket_bg: (58, 36, 90),
        bracket_fg: (236, 72, 153),
        bar_bg: (15, 13, 22),
        tab_active_bg: (34, 26, 52),
        ok: (248, 250, 252),
        warn: (255, 214, 0),
        err: (255, 30, 102),
        kw: (192, 132, 252),
        kw_bold: true,
        ty: (236, 72, 153),
        func: (248, 250, 252),
        str_col: (244, 114, 182),
        escape: (236, 72, 153),
        num: (168, 85, 247),
        constant: (168, 85, 247),
        param: (240, 171, 252),
        prop: (216, 180, 254),
        comment: (116, 102, 142),
        op: (236, 72, 153),
        punct: (176, 166, 200),
        var: (216, 180, 254),
        attr: (248, 250, 252),
        key: (192, 132, 252),
        heading: (216, 180, 254),
    },
    // Exact 1:1 match to VS Code "Best Themes - Dracula Redefined" + user settings.json overrides
    Palette {
        name: "dracula",
        cursor_hex: "#ff73c6",
        grad: [(255, 115, 198), (184, 130, 255), (107, 227, 245)], // #ff73c6 -> #b882ff -> #6be3f5
        fg: (235, 238, 247),                                      // #ebeef7 editor.foreground / variables
        dim: (121, 129, 161),                                     // #7981a1 tab.inactiveForeground
        line_num: (87, 94, 125),                                  // #575e7d editorLineNumber.foreground
        active_line_num: (199, 158, 255),                         // #c79eff editorLineNumber.activeForeground
        border: (46, 51, 71),                                     // #2e3347 editorWidget.border
        indent_guide: (46, 51, 71),                               // #2e3347 indent guide (readable on translucent bg)
        indent_active: (147, 112, 230),                           // #9370e6 editorIndentGuide.activeBackground1
        cur_line_bg: (28, 31, 46),                                // #1c1f2e editor.lineHighlightBackground
        sel_bg: (59, 42, 94),                                     // #3b2a5e editor.selectionBackground
        word_hl_bg: (30, 50, 66),                                 // #6be3f51e over #141620 wordHighlightBackground
        match_bg: (34, 56, 74),                                   // #6be3f524 over #141620 findMatchHighlightBackground
        active_match_bg: (120, 46, 96),                           // #ff73c640 vivid findMatchBackground
        bracket_bg: (34, 62, 78),                                 // #6be3f51c over #141620 editorBracketMatch.background
        bracket_fg: (107, 227, 245),                              // #6be3f5 editorBracketMatch.border
        bar_bg: (16, 17, 26),                                     // #10111a sideBar / tabsBackground
        tab_active_bg: (28, 31, 46),                              // #1c1f2e active tab
        ok: (92, 242, 152),                                       // #5cf298 terminal.ansiGreen
        warn: (242, 229, 143),                                    // #f2e58f terminal.ansiYellow
        err: (255, 97, 125),                                      // #ff617d terminal.ansiRed
        kw: (255, 115, 198),                                      // #ff73c6 keywords / storage.type / storage.modifier
        kw_bold: false,
        ty: (107, 227, 245),                                      // #6be3f5 entity.name.type / class / support.type
        func: (92, 242, 152),                                     // #5cf298 entity.name.function / support.function
        str_col: (242, 229, 143),                                 // #f2e58f string / string.quoted
        escape: (255, 115, 198),                                  // #ff73c6 constant.character.escape
        num: (199, 158, 255),                                     // #c79eff constant.numeric
        constant: (199, 158, 255),                                // #c79eff constant.language / constant.other
        param: (255, 181, 122),                                   // #ffb57a variable.parameter (italic)
        prop: (135, 230, 216),                                    // #87e6d8 variable.other.property / property-name
        comment: (127, 135, 168),                                 // #7f87a8 comments (italic)
        op: (255, 115, 198),                                      // #ff73c6 keyword.operator
        punct: (184, 192, 222),                                   // #b8c0de punctuation / meta.brace
        var: (199, 158, 255),                                     // #c79eff source.shell variable.other
        attr: (135, 230, 216),                                    // #87e6d8 entity.other.attribute-name
        key: (135, 230, 216),                                     // #87e6d8 support.type.property-name
        heading: (199, 158, 255),                                 // #c79eff markup.heading (bold)
    },
];

impl Palette {
    pub fn by_name(name: &str) -> &'static Palette {
        PALETTES
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name.trim()))
            .unwrap_or(&PALETTES[5])
    }

    pub fn by_index(idx: usize) -> &'static Palette {
        &PALETTES[idx % PALETTES.len()]
    }

    pub fn load_for_editor(configured: &str) -> &'static Palette {
        let trimmed = configured.trim();
        if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("auto") {
            return Self::by_name(trimmed);
        }
        let path = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".config")
            })
            .join("nave/palette");
        if let Ok(name) = fs::read_to_string(path) {
            Self::by_name(&name)
        } else {
            Self::by_name("dracula")
        }
    }

    pub fn token_style(&self, kind: TokenKind) -> TokenStyle {
        match kind {
            TokenKind::Plain => TokenStyle { fg: self.fg, bold: false, italic: false },
            TokenKind::Keyword => TokenStyle { fg: self.kw, bold: self.kw_bold, italic: false },
            TokenKind::Type => TokenStyle { fg: self.ty, bold: false, italic: false },
            TokenKind::Function => TokenStyle { fg: self.func, bold: false, italic: false },
            TokenKind::String => TokenStyle { fg: self.str_col, bold: false, italic: false },
            TokenKind::Escape => TokenStyle { fg: self.escape, bold: false, italic: false },
            TokenKind::Number => TokenStyle { fg: self.num, bold: false, italic: false },
            TokenKind::Constant => TokenStyle { fg: self.constant, bold: false, italic: false },
            TokenKind::BuiltinVar => TokenStyle { fg: self.constant, bold: false, italic: true },
            TokenKind::Parameter => TokenStyle { fg: self.param, bold: false, italic: true },
            TokenKind::Property => TokenStyle { fg: self.prop, bold: false, italic: false },
            TokenKind::Comment => TokenStyle { fg: self.comment, bold: false, italic: true },
            TokenKind::Operator => TokenStyle { fg: self.op, bold: false, italic: false },
            TokenKind::Punctuation => TokenStyle { fg: self.punct, bold: false, italic: false },
            TokenKind::Variable => TokenStyle { fg: self.var, bold: false, italic: false },
            TokenKind::Attribute => TokenStyle { fg: self.attr, bold: false, italic: false },
            TokenKind::Key => TokenStyle { fg: self.key, bold: false, italic: false },
            TokenKind::Heading => TokenStyle { fg: self.heading, bold: true, italic: false },
        }
    }
}

pub fn lerp_grad(stops: [Rgb; 3], t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let (a, b, u) = if t < 0.5 {
        (stops[0], stops[1], t * 2.0)
    } else {
        (stops[1], stops[2], (t - 0.5) * 2.0)
    };
    (
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * u) as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * u) as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * u) as u8,
    )
}

pub fn push_fg(out: &mut String, c: Rgb, bold: bool) {
    if bold {
        let _ = write!(out, "\x1b[1;23;38;2;{};{};{}m", c.0, c.1, c.2);
    } else {
        let _ = write!(out, "\x1b[22;23;38;2;{};{};{}m", c.0, c.1, c.2);
    }
}

pub fn push_fg_styled(out: &mut String, c: Rgb, bold: bool, italic: bool) {
    let b = if bold { 1 } else { 22 };
    let it = if italic { 3 } else { 23 };
    let _ = write!(out, "\x1b[{};{};38;2;{};{};{}m", b, it, c.0, c.1, c.2);
}

pub fn push_bg(out: &mut String, c: Rgb) {
    let _ = write!(out, "\x1b[48;2;{};{};{}m", c.0, c.1, c.2);
}

pub fn push_grad_text(out: &mut String, text: &str, stops: [Rgb; 3], bold: bool) {
    let total = text.chars().count().saturating_sub(1).max(1) as f32;
    for (i, ch) in text.chars().enumerate() {
        if ch == ' ' {
            out.push(' ');
        } else {
            let c = lerp_grad(stops, i as f32 / total);
            push_fg(out, c, bold);
            out.push(ch);
        }
    }
}
