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
    Number,
    Comment,
    Operator,
    Variable,
    Attribute,
    Key,
    Heading,
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub name: &'static str,
    pub grad: [Rgb; 3],
    pub fg: Rgb,
    pub dim: Rgb,
    pub border: Rgb,
    pub cur_line_bg: Rgb,
    pub sel_bg: Rgb,
    pub match_bg: Rgb,
    pub active_match_bg: Rgb,
    pub bracket_bg: Rgb,
    pub bar_bg: Rgb,
    pub tab_active_bg: Rgb,
    pub ok: Rgb,
    pub warn: Rgb,
    pub err: Rgb,
    pub kw: Rgb,
    pub ty: Rgb,
    pub func: Rgb,
    pub str_col: Rgb,
    pub num: Rgb,
    pub comment: Rgb,
    pub op: Rgb,
    pub var: Rgb,
    pub attr: Rgb,
    pub key: Rgb,
}

pub const PALETTES: [Palette; 5] = [
    Palette {
        name: "cyber",
        grad: [(0, 245, 255), (124, 77, 255), (255, 46, 151)],
        fg: (241, 245, 249),
        dim: (100, 108, 142),
        border: (59, 47, 99),
        cur_line_bg: (20, 23, 38),
        sel_bg: (42, 54, 96),
        match_bg: (56, 48, 84),
        active_match_bg: (124, 77, 255),
        bracket_bg: (54, 66, 108),
        bar_bg: (16, 18, 30),
        tab_active_bg: (28, 32, 54),
        ok: (0, 255, 136),
        warn: (255, 196, 0),
        err: (255, 30, 102),
        kw: (192, 132, 252),
        ty: (0, 245, 255),
        func: (86, 204, 255),
        str_col: (255, 214, 0),
        num: (255, 118, 178),
        comment: (106, 114, 148),
        op: (255, 46, 151),
        var: (125, 249, 255),
        attr: (0, 255, 136),
        key: (0, 240, 255),
    },
    Palette {
        name: "emerald",
        grad: [(0, 255, 136), (0, 229, 255), (178, 255, 89)],
        fg: (236, 253, 245),
        dim: (78, 126, 98),
        border: (31, 89, 59),
        cur_line_bg: (14, 29, 20),
        sel_bg: (26, 72, 48),
        match_bg: (30, 66, 54),
        active_match_bg: (0, 140, 96),
        bracket_bg: (38, 86, 60),
        bar_bg: (11, 22, 16),
        tab_active_bg: (20, 44, 30),
        ok: (0, 255, 136),
        warn: (255, 234, 0),
        err: (255, 30, 102),
        kw: (0, 255, 136),
        ty: (0, 229, 255),
        func: (120, 255, 180),
        str_col: (255, 234, 0),
        num: (178, 255, 89),
        comment: (84, 130, 104),
        op: (0, 229, 255),
        var: (167, 243, 208),
        attr: (178, 255, 89),
        key: (0, 245, 212),
    },
    Palette {
        name: "sunset",
        grad: [(255, 214, 0), (255, 94, 58), (255, 30, 142)],
        fg: (255, 241, 242),
        dim: (140, 84, 112),
        border: (92, 37, 64),
        cur_line_bg: (30, 16, 24),
        sel_bg: (82, 32, 56),
        match_bg: (76, 42, 34),
        active_match_bg: (190, 52, 88),
        bracket_bg: (92, 46, 62),
        bar_bg: (22, 12, 18),
        tab_active_bg: (48, 22, 36),
        ok: (0, 255, 136),
        warn: (255, 214, 0),
        err: (255, 23, 68),
        kw: (255, 46, 151),
        ty: (255, 159, 28),
        func: (255, 118, 82),
        str_col: (255, 214, 0),
        num: (255, 94, 178),
        comment: (146, 92, 118),
        op: (255, 94, 58),
        var: (255, 198, 128),
        attr: (255, 214, 0),
        key: (255, 140, 90),
    },
    Palette {
        name: "nord",
        grad: [(56, 232, 255), (108, 140, 255), (192, 132, 252)],
        fg: (241, 245, 249),
        dim: (92, 112, 150),
        border: (44, 63, 102),
        cur_line_bg: (18, 26, 43),
        sel_bg: (42, 64, 108),
        match_bg: (46, 58, 92),
        active_match_bg: (90, 116, 210),
        bracket_bg: (52, 74, 118),
        bar_bg: (13, 19, 32),
        tab_active_bg: (26, 38, 64),
        ok: (52, 255, 180),
        warn: (255, 215, 95),
        err: (255, 95, 115),
        kw: (192, 132, 252),
        ty: (56, 232, 255),
        func: (108, 168, 255),
        str_col: (255, 215, 95),
        num: (165, 180, 252),
        comment: (96, 116, 154),
        op: (108, 140, 255),
        var: (147, 235, 255),
        attr: (52, 255, 180),
        key: (56, 232, 255),
    },
    Palette {
        name: "stealth",
        grad: [(248, 250, 252), (168, 85, 247), (236, 72, 153)],
        fg: (244, 244, 248),
        dim: (112, 98, 138),
        border: (76, 40, 117),
        cur_line_bg: (21, 18, 31),
        sel_bg: (64, 38, 98),
        match_bg: (58, 40, 82),
        active_match_bg: (148, 68, 224),
        bracket_bg: (76, 48, 112),
        bar_bg: (15, 13, 22),
        tab_active_bg: (34, 26, 52),
        ok: (248, 250, 252),
        warn: (255, 214, 0),
        err: (255, 30, 102),
        kw: (192, 132, 252),
        ty: (236, 72, 153),
        func: (248, 250, 252),
        str_col: (244, 114, 182),
        num: (168, 85, 247),
        comment: (116, 102, 142),
        op: (236, 72, 153),
        var: (216, 180, 254),
        attr: (248, 250, 252),
        key: (192, 132, 252),
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

    pub fn load_active() -> &'static Palette {
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

    pub fn token_color(&self, kind: TokenKind) -> (Rgb, bool) {
        match kind {
            TokenKind::Plain => (self.fg, false),
            TokenKind::Keyword => (self.kw, true),
            TokenKind::Type => (self.ty, false),
            TokenKind::Function => (self.func, false),
            TokenKind::String => (self.str_col, false),
            TokenKind::Number => (self.num, false),
            TokenKind::Comment => (self.comment, false),
            TokenKind::Operator => (self.op, false),
            TokenKind::Variable => (self.var, false),
            TokenKind::Attribute => (self.attr, false),
            TokenKind::Key => (self.key, false),
            TokenKind::Heading => (self.grad[0], true),
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
        let _ = write!(out, "\x1b[1;38;2;{};{};{}m", c.0, c.1, c.2);
    } else {
        let _ = write!(out, "\x1b[22;38;2;{};{};{}m", c.0, c.1, c.2);
    }
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
