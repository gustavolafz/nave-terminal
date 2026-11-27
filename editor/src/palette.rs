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
    // 1. Nave Cyber (Obsidian Ciano / Violeta / Magenta — padrão Nave Terminal)
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
        word_hl_bg: (56, 48, 84),
        match_bg: (56, 48, 84),
        active_match_bg: (124, 77, 255),
        bracket_bg: (0, 245, 255),
        bracket_fg: (16, 18, 30),
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
        param: (255, 176, 92),
        prop: (125, 249, 255),
        comment: (106, 114, 148),
        op: (255, 46, 151),
        punct: (148, 158, 194),
        var: (125, 249, 255),
        attr: (0, 255, 136),
        key: (0, 240, 255),
        heading: (0, 245, 255),
    },
    // 2. Nave Emerald (Obsidian Esmeralda / Aqua / Lima)
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
        word_hl_bg: (30, 66, 54),
        match_bg: (30, 66, 54),
        active_match_bg: (0, 140, 96),
        bracket_bg: (0, 255, 136),
        bracket_fg: (11, 22, 16),
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
        param: (255, 218, 102),
        prop: (167, 243, 208),
        comment: (84, 130, 104),
        op: (0, 229, 255),
        punct: (132, 182, 154),
        var: (167, 243, 208),
        attr: (178, 255, 89),
        key: (0, 245, 212),
        heading: (0, 255, 136),
    },
    // 3. Nave Sunset (Obsidian Ouro / Magma / Magenta)
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
        word_hl_bg: (76, 42, 34),
        match_bg: (76, 42, 34),
        active_match_bg: (190, 52, 88),
        bracket_bg: (255, 214, 0),
        bracket_fg: (22, 12, 18),
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
        prop: (255, 159, 96),
        comment: (146, 92, 118),
        op: (255, 94, 58),
        punct: (196, 142, 164),
        var: (255, 198, 128),
        attr: (255, 214, 0),
        key: (255, 140, 90),
        heading: (255, 214, 0),
    },
    // 4. Nave Nord (Obsidian Aurora Gelo / Azul / Violeta)
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
        word_hl_bg: (46, 58, 92),
        match_bg: (46, 58, 92),
        active_match_bg: (90, 116, 210),
        bracket_bg: (56, 232, 255),
        bracket_fg: (13, 19, 32),
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
        param: (255, 192, 128),
        prop: (147, 235, 255),
        comment: (96, 116, 154),
        op: (108, 140, 255),
        punct: (148, 166, 200),
        var: (147, 235, 255),
        attr: (52, 255, 180),
        key: (56, 232, 255),
        heading: (56, 232, 255),
    },
    // 5. Nave Stealth (Obsidian Ametista / Monocromático Violeta)
    Palette {
        name: "stealth",
        cursor_hex: "#a855f7",
        grad: [(248, 250, 252), (168, 85, 247), (236, 72, 153)],
        fg: (244, 244, 248),
        dim: (112, 98, 138),
        line_num: (112, 98, 138),
        active_line_num: (248, 250, 252),
        border: (76, 40, 117),
        indent_guide: (76, 40, 117),
        indent_active: (168, 85, 247),
        cur_line_bg: (21, 18, 31),
        sel_bg: (64, 38, 98),
        word_hl_bg: (58, 40, 82),
        match_bg: (58, 40, 82),
        active_match_bg: (148, 68, 224),
        bracket_bg: (248, 250, 252),
        bracket_fg: (15, 13, 22),
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
        punct: (164, 152, 192),
        var: (216, 180, 254),
        attr: (248, 250, 252),
        key: (192, 132, 252),
        heading: (248, 250, 252),
    },
    // 6. Dracula Redefined (VS Code customizado salvo como opção extra — Alt+6)
    Palette {
        name: "dracula",
        cursor_hex: "#ff73c6",
        grad: [(255, 115, 198), (184, 130, 255), (107, 227, 245)],
        fg: (235, 238, 247),
        dim: (121, 129, 161),
        line_num: (87, 94, 125),
        active_line_num: (199, 158, 255),
        border: (46, 51, 71),
        indent_guide: (46, 51, 71),
        indent_active: (147, 112, 230),
        cur_line_bg: (28, 31, 46),
        sel_bg: (59, 42, 94),
        word_hl_bg: (30, 50, 66),
        match_bg: (34, 56, 74),
        active_match_bg: (120, 46, 96),
        bracket_bg: (34, 62, 78),
        bracket_fg: (107, 227, 245),
        bar_bg: (16, 17, 26),
        tab_active_bg: (28, 31, 46),
        ok: (92, 242, 152),
        warn: (242, 229, 143),
        err: (255, 97, 125),
        kw: (255, 115, 198),
        kw_bold: false,
        ty: (107, 227, 245),
        func: (92, 242, 152),
        str_col: (242, 229, 143),
        escape: (255, 115, 198),
        num: (199, 158, 255),
        constant: (199, 158, 255),
        param: (255, 181, 122),
        prop: (135, 230, 216),
        comment: (127, 135, 168),
        op: (255, 115, 198),
        punct: (184, 192, 222),
        var: (199, 158, 255),
        attr: (135, 230, 216),
        key: (135, 230, 216),
        heading: (199, 158, 255),
    },
];

impl Palette {
    pub fn by_name(name: &str) -> &'static Palette {
        PALETTES
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name.trim()))
            .unwrap_or(&PALETTES[0])
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
            &PALETTES[0]
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
