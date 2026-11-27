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
    pub ty_italic: bool,
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
    // 1. Nave Cyber — Electric Neon Cyberpunk (Ciano · Violeta Elétrico · Menta · Ouro · Magenta)
    Palette {
        name: "cyber",
        cursor_hex: "#00f8ff",
        grad: [(0, 248, 255), (156, 98, 255), (255, 56, 162)],
        fg: (244, 248, 255),
        dim: (116, 126, 164),
        line_num: (112, 122, 158),
        active_line_num: (0, 248, 255),
        border: (64, 52, 108),
        indent_guide: (64, 52, 108),
        indent_active: (146, 96, 255),
        cur_line_bg: (20, 23, 38),
        sel_bg: (44, 56, 102),
        word_hl_bg: (56, 48, 86),
        match_bg: (56, 48, 86),
        active_match_bg: (132, 82, 255),
        bracket_bg: (0, 248, 255),
        bracket_fg: (14, 16, 28),
        bar_bg: (16, 18, 30),
        tab_active_bg: (28, 32, 54),
        ok: (48, 255, 168),
        warn: (255, 214, 0),
        err: (255, 42, 114),
        kw: (216, 150, 255),        // #D896FF electric neon violet (bold)
        kw_bold: true,
        ty: (0, 248, 255),          // #00F8FF hyper neon cyan
        ty_italic: false,
        func: (48, 255, 168),       // #30FFA8 electric neon mint
        str_col: (255, 226, 52),    // #FFE234 electric solar gold
        escape: (255, 56, 162),     // #FF38A2 laser magenta
        num: (255, 122, 198),       // #FF7AC6 vibrant neon pink
        constant: (255, 122, 198),  // #FF7AC6 vibrant neon pink
        param: (255, 184, 77),      // #FFB84D electric amber (italic)
        prop: (102, 218, 255),      // #66DAFF electric sky azure
        comment: (124, 134, 174),   // #7C86AE readable cool slate (italic)
        op: (255, 56, 162),         // #FF38A2 laser magenta
        punct: (216, 226, 242),     // #D8E2F2 crisp bright silver
        var: (138, 248, 255),       // #8AF8FF electric ice cyan
        attr: (176, 255, 78),       // #B0FF4E electric lime
        key: (102, 218, 255),       // #66DAFF electric sky azure
        heading: (0, 248, 255),     // #00F8FF hyper cyan (bold)
    },
    // 2. Nave Emerald — Bioluminescent Tropical (Esmeralda · Aqua · Lima · Ouro · Ametista · Coral)
    Palette {
        name: "emerald",
        cursor_hex: "#00ff94",
        grad: [(0, 255, 148), (0, 236, 255), (176, 255, 82)],
        fg: (240, 254, 248),
        dim: (94, 144, 116),
        line_num: (94, 144, 116),
        active_line_num: (0, 255, 148),
        border: (36, 98, 66),
        indent_guide: (36, 98, 66),
        indent_active: (0, 236, 255),
        cur_line_bg: (14, 29, 20),
        sel_bg: (28, 76, 52),
        word_hl_bg: (30, 68, 54),
        match_bg: (30, 68, 54),
        active_match_bg: (0, 148, 102),
        bracket_bg: (0, 255, 148),
        bracket_fg: (11, 22, 16),
        bar_bg: (11, 22, 16),
        tab_active_bg: (20, 44, 30),
        ok: (0, 255, 148),
        warn: (255, 228, 74),
        err: (255, 58, 118),
        kw: (0, 255, 148),          // #00FF94 electric emerald (bold)
        kw_bold: true,
        ty: (0, 236, 255),          // #00ECFF electric aqua
        ty_italic: false,
        func: (176, 255, 82),       // #B0FF52 bioluminescent lime
        str_col: (255, 228, 74),    // #FFE44A vibrant tropical gold
        escape: (255, 92, 168),     // #FF5CA8 vibrant orchid coral
        num: (196, 152, 255),       // #C498FF electric amethyst
        constant: (196, 152, 255),  // #C498FF electric amethyst
        param: (255, 176, 92),      // #FFB05C warm mango (italic)
        prop: (94, 242, 224),       // #5EF2E0 bright seafoam teal
        comment: (102, 152, 124),   // #66987C forest slate (italic)
        op: (255, 92, 168),         // #FF5CA8 orchid coral contrast
        punct: (204, 232, 218),     // #CCE8DA crisp mint silver
        var: (140, 255, 214),       // #8CFFD6 bright mint
        attr: (255, 138, 198),      // #FF8AC6 orchid blossom
        key: (94, 242, 224),        // #5EF2E0 seafoam teal
        heading: (0, 255, 148),     // #00FF94 electric emerald (bold)
    },
    // 3. Nave Sunset — Electric Synthwave (Magenta · Magma · Ouro · Ciano Crepúsculo · Violeta · Menta)
    Palette {
        name: "sunset",
        cursor_hex: "#ff5e3a",
        grad: [(255, 220, 40), (255, 102, 64), (255, 54, 156)],
        fg: (255, 244, 246),
        dim: (158, 102, 130),
        line_num: (158, 102, 130),
        active_line_num: (255, 220, 40),
        border: (98, 42, 70),
        indent_guide: (98, 42, 70),
        indent_active: (255, 102, 64),
        cur_line_bg: (30, 16, 24),
        sel_bg: (86, 34, 60),
        word_hl_bg: (78, 42, 36),
        match_bg: (78, 42, 36),
        active_match_bg: (196, 54, 94),
        bracket_bg: (255, 220, 40),
        bracket_fg: (22, 12, 18),
        bar_bg: (22, 12, 18),
        tab_active_bg: (48, 22, 36),
        ok: (98, 255, 192),
        warn: (255, 220, 40),
        err: (255, 42, 86),
        kw: (255, 62, 158),         // #FF3E9E electric hot magenta (bold)
        kw_bold: true,
        ty: (76, 232, 255),         // #4CE8FF twilight neon cyan
        ty_italic: false,
        func: (255, 178, 56),       // #FFB238 electric golden amber
        str_col: (255, 236, 92),    // #FFEC5C bright solar yellow
        escape: (255, 62, 158),     // #FF3E9E hot magenta
        num: (206, 144, 255),       // #CE90FF synthwave violet
        constant: (206, 144, 255),  // #CE90FF synthwave violet
        param: (255, 136, 108),     // #FF886C warm sunset coral (italic)
        prop: (98, 255, 192),       // #62FFC0 electric oasis mint
        comment: (164, 108, 136),   // #A46C88 warm rose slate (italic)
        op: (255, 112, 67),         // #FF7043 vibrant magma
        punct: (234, 206, 218),     // #EACEDA crisp rose silver
        var: (255, 202, 138),       // #FFCA8A warm sunset peach
        attr: (255, 128, 212),      // #FF80D4 neon flamingo
        key: (98, 255, 192),        // #62FFC0 oasis mint
        heading: (255, 220, 40),    // #FFDC28 solar gold (bold)
    },
    // 4. Nave Nord — Electric Aurora Borealis (Violeta · Ciano Glacial · Menta Aurora · Ouro · Rosa)
    Palette {
        name: "nord",
        cursor_hex: "#40eeff",
        grad: [(64, 238, 255), (112, 152, 255), (204, 146, 255)],
        fg: (242, 247, 255),
        dim: (108, 130, 170),
        line_num: (108, 130, 170),
        active_line_num: (64, 238, 255),
        border: (48, 70, 112),
        indent_guide: (48, 70, 112),
        indent_active: (112, 152, 255),
        cur_line_bg: (18, 26, 43),
        sel_bg: (44, 68, 114),
        word_hl_bg: (46, 60, 96),
        match_bg: (46, 60, 96),
        active_match_bg: (94, 122, 218),
        bracket_bg: (64, 238, 255),
        bracket_fg: (13, 19, 32),
        bar_bg: (13, 19, 32),
        tab_active_bg: (26, 38, 64),
        ok: (74, 255, 188),
        warn: (255, 224, 102),
        err: (255, 98, 126),
        kw: (204, 146, 255),        // #CC92FF electric aurora violet (bold)
        kw_bold: true,
        ty: (64, 238, 255),         // #40EEFF glacial neon cyan
        ty_italic: false,
        func: (74, 255, 188),       // #4AFFBC electric aurora green
        str_col: (255, 224, 102),   // #FFE066 bright aurora gold
        escape: (255, 114, 156),    // #FF729C arctic rose
        num: (255, 126, 178),       // #FF7EB2 vibrant aurora pink
        constant: (255, 126, 178),  // #FF7EB2 vibrant aurora pink
        param: (255, 186, 116),     // #FFBA74 warm amber (italic)
        prop: (132, 216, 255),      // #84D8FF frost sky blue
        comment: (114, 136, 176),   // #7288B0 polar slate (italic)
        op: (112, 166, 255),        // #70A6FF electric arctic blue
        punct: (206, 218, 238),     // #CEDAEE crisp glacial silver
        var: (166, 240, 255),       // #A6F0FF pale ice
        attr: (168, 255, 120),      // #A8FF78 aurora lime
        key: (132, 216, 255),       // #84D8FF frost sky blue
        heading: (64, 238, 255),    // #40EEFF glacial cyan (bold)
    },
    // 5. Nave Stealth — Electric Amethyst Jewel (Ametista · Rubi · Safira · Jade · Ouro · Orquídea)
    Palette {
        name: "stealth",
        cursor_hex: "#ce8aff",
        grad: [(246, 245, 252), (192, 112, 255), (255, 86, 172)],
        fg: (246, 245, 252),
        dim: (128, 112, 158),
        line_num: (128, 112, 158),
        active_line_num: (246, 245, 252),
        border: (82, 46, 126),
        indent_guide: (82, 46, 126),
        indent_active: (184, 102, 255),
        cur_line_bg: (21, 18, 31),
        sel_bg: (68, 40, 104),
        word_hl_bg: (60, 42, 86),
        match_bg: (60, 42, 86),
        active_match_bg: (156, 74, 232),
        bracket_bg: (246, 245, 252),
        bracket_fg: (15, 13, 22),
        bar_bg: (15, 13, 22),
        tab_active_bg: (34, 26, 52),
        ok: (104, 255, 198),
        warn: (255, 220, 112),
        err: (255, 52, 116),
        kw: (206, 138, 255),        // #CE8AFF electric amethyst (bold)
        kw_bold: true,
        ty: (88, 232, 255),         // #58E8FF electric sapphire cyan
        ty_italic: false,
        func: (104, 255, 198),      // #68FFC6 vibrant jade mint
        str_col: (255, 220, 112),   // #FFDC70 bright champagne gold
        escape: (255, 86, 172),     // #FF56AC neon ruby rose
        num: (255, 138, 216),       // #FF8AD8 vivid orchid pink
        constant: (255, 138, 216),  // #FF8AD8 vivid orchid pink
        param: (255, 182, 126),     // #FFB67E warm rose-gold peach (italic)
        prop: (164, 198, 255),      // #A4C6FF electric periwinkle
        comment: (134, 118, 166),   // #8676A6 soft amethyst slate (italic)
        op: (255, 86, 172),         // #FF56AC neon ruby rose
        punct: (212, 204, 232),     // #D4CCE8 crisp lilac silver
        var: (222, 190, 255),       // #DEBEFF bright lavender
        attr: (182, 255, 118),      // #B6FF76 electric chartreuse
        key: (164, 198, 255),       // #A4C6FF electric periwinkle
        heading: (216, 170, 255),   // #D8AAFF bright amethyst (bold)
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
        ty_italic: true,
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
            TokenKind::Type => TokenStyle { fg: self.ty, bold: false, italic: self.ty_italic },
            TokenKind::Function => TokenStyle { fg: self.func, bold: false, italic: false },
            TokenKind::String => TokenStyle { fg: self.str_col, bold: false, italic: false },
            TokenKind::Escape => TokenStyle { fg: self.escape, bold: false, italic: false },
            TokenKind::Number => TokenStyle { fg: self.num, bold: false, italic: false },
            TokenKind::Constant => TokenStyle { fg: self.constant, bold: false, italic: false },
            TokenKind::BuiltinVar => TokenStyle { fg: self.constant, bold: false, italic: false },
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_palettes_have_distinct_vibrant_syntax_roles() {
        for p in &PALETTES {
            let distinct: HashSet<Rgb> = [
                p.fg, p.kw, p.ty, p.func, p.str_col, p.num, p.param, p.prop, p.comment, p.punct,
            ]
            .into_iter()
            .collect();
            assert!(
                distinct.len() >= 10,
                "Palette '{}' only has {} distinct colors across 10 core token roles!",
                p.name,
                distinct.len()
            );
        }
    }
}
