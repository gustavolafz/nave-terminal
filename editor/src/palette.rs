use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

pub type Rgb = (u8, u8, u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Plain,
    ControlFlow,   // if, else, while, for, loop, match, switch, case, return, break, continue, try, catch, throw, raise, yield, await
    Keyword,       // declarations & modifiers: const, let, mut, pub, fn, def, class, struct, enum, impl, using, namespace, import, auto...
    PrimitiveType, // int, long, double, float, bool, char, void, usize, i32, u8, f64, str, string, number, boolean, byte, rune, error
    Type,          // STL / classes / containers / generics / lifetimes: vector, pair, String, Vec, Option, Result, Node, T, 'a
    Function,      // functions, methods, calls, builtins, shell commands
    Macro,         // println!, vec!, #include, #define, #pragma, @decorator, @Override
    StdBuiltin,    // cin, cout, cerr, endl, std, console, Math, JSON, process, window, document, fmt, System
    String,        // string body
    Escape,        // \n, \t, \x1b, \"
    Number,        // numeric digits
    NumUnit,       // 0x, 0b, 0o, e, E, p, P, LL, ull, f, usize, f64 (non-bold)
    Constant,      // true, false, None, null, nullptr, undefined, nil, self/this/super/Self, flags, %d/%s placeholders
    ConstIdent,    // SCREAMING_SNAKE_CASE constants: INF, MOD, MAXN, MAX_LEN, PI, EOF
    Parameter,     // function / method / lambda parameters (italic)
    Property,      // .field, ->val, CSS properties
    Key,           // JSON / YAML / TOML keys
    Attribute,     // HTML / JSX attributes
    Variable,      // shell $VAR / ${VAR}
    Operator,      // =, +, -, ==, !=, <<, >>, ++, --, &&, ||, ->, =>
    Punctuation,   // , ; . : :: (and brackets when rainbow_brackets is OFF)
    Rainbow(u8),   // 0..=3 cross-line nesting depth for () [] {} and generic <>
    BracketError,  // unmatched closing ) ] }
    Comment,       // normal comment (italic)
    DocComment,    // ///, /** */, Python docstrings (brighter italic)
    TodoBadge,     // TODO:, NOTE:, PERF:, OPTIMIZE: badge inside comments
    FixmeBadge,    // FIXME:, BUG:, HACK:, WARN:, XXX: badge inside comments
    Heading,       // Markdown # Heading, TOML [section]
}

#[derive(Clone, Copy, Debug)]
pub struct TokenStyle {
    pub fg: Rgb,
    pub bg: Option<Rgb>,
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
    pub ctrl: Rgb,
    pub kw: Rgb,
    pub kw_bold: bool,
    pub prim_ty: Rgb,
    pub ty: Rgb,
    pub ty_italic: bool,
    pub func: Rgb,
    pub mac: Rgb,
    pub std_bi: Rgb,
    pub str_col: Rgb,
    pub escape: Rgb,
    pub num: Rgb,
    pub num_unit: Rgb,
    pub constant: Rgb,
    pub const_ident: Rgb,
    pub param: Rgb,
    pub prop: Rgb,
    pub comment: Rgb,
    pub doc_comment: Rgb,
    pub op: Rgb,
    pub punct: Rgb,
    pub var: Rgb,
    pub attr: Rgb,
    pub key: Rgb,
    pub heading: Rgb,
    pub rainbow: [Rgb; 4],
    pub todo_bg: Rgb,
    pub fixme_bg: Rgb,
}

pub const PALETTES: [Palette; 6] = [
    // 1. Nave Cyber — Signature Electric Cyberpunk (Hiper-Ciano · Rosa-Magenta · Violeta · Menta · Ouro · Lima · Azul-Céu)
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
        warn: (255, 226, 52),
        err: (255, 42, 114),
        ctrl: (255, 63, 164),       // #FF3FA4 bold electric rose-magenta (if/while/for/return)
        kw: (212, 148, 255),        // #D494FF bold electric violet (const/fn/struct/using)
        kw_bold: true,
        prim_ty: (0, 248, 255),     // #00F8FF hyper-cyan (int/long/bool/usize/str)
        ty: (96, 244, 255),         // #60F4FF electric ice-cyan (vector/String/Option/Node)
        ty_italic: false,
        func: (48, 255, 168),       // #30FFA8 electric neon mint (main/min/sort)
        mac: (184, 255, 74),        // #B8FF4A electric lime (#include/#define/println!/@decorator)
        std_bi: (102, 212, 255),    // #66D4FF electric sky-azure (cin/cout/endl/console/std)
        str_col: (255, 226, 52),    // #FFE234 solar gold
        escape: (255, 63, 164),     // #FF3FA4 laser magenta
        num: (255, 120, 184),       // #FF78B8 neon coral-pink
        num_unit: (212, 148, 255),  // #D494FF non-bold violet (e/0x/LL/usize)
        constant: (199, 158, 255),  // #C79EFF neon lavender (true/false/null/self/%d)
        const_ident: (255, 150, 230), // #FF96E6 luminous orchid (INF/MOD/MAXN/MAX_LEN)
        param: (255, 179, 71),      // #FFB347 electric amber (italic)
        prop: (125, 249, 255),      // #7DF9FF electric aqua (.field)
        comment: (126, 138, 180),   // #7E8AB4 readable slate (italic)
        doc_comment: (158, 173, 216), // #9EADD8 bright silver-slate (italic)
        op: (255, 56, 162),         // #FF38A2 laser magenta
        punct: (212, 223, 242),     // #D4DFF2 crisp bright silver
        var: (138, 248, 255),       // #8AF8FF ice neon ($VAR)
        attr: (125, 249, 255),      // #7DF9FF electric aqua
        key: (102, 218, 255),       // #66DAFF sky azure
        heading: (0, 248, 255),     // #00F8FF hyper-cyan (bold)
        rainbow: [
            (255, 226, 52),  // depth 0: solar gold
            (212, 148, 255), // depth 1: electric violet
            (0, 248, 255),   // depth 2: hyper-cyan
            (48, 255, 168),  // depth 3: neon mint
        ],
        todo_bg: (0, 240, 255),     // electric cyan pill badge
        fixme_bg: (255, 56, 122),   // vivid crimson pill badge
    },
    // 2. Nave Emerald — Bioluminescent Tropical (Esmeralda · Coral · Aqua · Lima · Ouro · Ametista)
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
        ctrl: (255, 92, 168),       // #FF5CA8 vivid orchid-coral (control flow)
        kw: (0, 255, 148),          // #00FF94 electric emerald (declarations)
        kw_bold: true,
        prim_ty: (0, 236, 255),     // #00ECFF electric aqua
        ty: (94, 246, 255),         // #5EF6FF bright sea-cyan
        ty_italic: false,
        func: (176, 255, 82),       // #B0FF52 bioluminescent lime
        mac: (255, 196, 66),        // #FFC442 electric sunburst
        std_bi: (112, 242, 214),    // #70F2D6 bright seafoam
        str_col: (255, 228, 74),    // #FFE44A tropical gold
        escape: (255, 92, 168),     // #FF5CA8 coral pink
        num: (196, 152, 255),       // #C498FF electric amethyst
        num_unit: (0, 255, 148),    // #00FF94 emerald unit
        constant: (182, 142, 255),  // #B68EFF lavender
        const_ident: (255, 146, 204), // #FF92CC orchid blossom
        param: (255, 176, 92),      // #FFB05C warm mango (italic)
        prop: (94, 242, 224),       // #5EF2E0 bright teal
        comment: (102, 152, 124),   // #66987C forest slate (italic)
        doc_comment: (132, 186, 156), // #84BA9C bright moss-slate (italic)
        op: (255, 108, 174),        // #FF6CAE orchid-coral
        punct: (204, 232, 218),     // #CCE8DA crisp mint silver
        var: (140, 255, 214),       // #8CFFD6 bright mint
        attr: (255, 138, 198),      // #FF8AC6 orchid
        key: (94, 242, 224),        // #5EF2E0 seafoam
        heading: (0, 255, 148),     // #00FF94 emerald (bold)
        rainbow: [
            (255, 228, 74),  // depth 0: tropical gold
            (0, 236, 255),   // depth 1: electric aqua
            (255, 122, 188), // depth 2: orchid pink
            (176, 255, 82),  // depth 3: lime
        ],
        todo_bg: (0, 255, 148),
        fixme_bg: (255, 68, 128),
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
        ctrl: (255, 98, 62),        // #FF623E electric magma (control flow)
        kw: (255, 62, 158),         // #FF3E9E hot synthwave magenta (declarations)
        kw_bold: true,
        prim_ty: (76, 232, 255),    // #4CE8FF twilight neon cyan
        ty: (128, 240, 255),        // #80F0FF bright sky-cyan
        ty_italic: false,
        func: (255, 178, 56),       // #FFB238 electric golden amber
        mac: (98, 255, 192),        // #62FFC0 electric oasis mint
        std_bi: (124, 212, 255),    // #7CD4FF twilight azure
        str_col: (255, 236, 92),    // #FFEC5C bright solar yellow
        escape: (255, 62, 158),     // #FF3E9E hot magenta
        num: (206, 144, 255),       // #CE90FF synthwave violet
        num_unit: (255, 62, 158),   // #FF3E9E magenta unit
        constant: (194, 134, 255),  // #C286FF bright violet
        const_ident: (255, 142, 214), // #FF8ED6 neon flamingo
        param: (255, 146, 114),     // #FF9272 warm coral (italic)
        prop: (98, 255, 192),       // #62FFC0 oasis mint
        comment: (164, 108, 136),   // #A46C88 rose slate (italic)
        doc_comment: (198, 144, 172), // #C690AC bright mauve (italic)
        op: (255, 112, 67),         // #FF7043 vibrant magma
        punct: (234, 206, 218),     // #EACEDA crisp rose silver
        var: (255, 202, 138),       // #FFCA8A sunset peach
        attr: (255, 128, 212),      // #FF80D4 flamingo
        key: (98, 255, 192),        // #62FFC0 oasis mint
        heading: (255, 220, 40),    // #FFDC28 solar gold (bold)
        rainbow: [
            (255, 220, 40),  // depth 0: solar gold
            (255, 62, 158),  // depth 1: hot magenta
            (76, 232, 255),  // depth 2: twilight cyan
            (98, 255, 192),  // depth 3: oasis mint
        ],
        todo_bg: (255, 214, 48),
        fixme_bg: (255, 52, 98),
    },
    // 4. Nave Nord — Electric Aurora Borealis (Violeta · Rosa Aurora · Ciano Glacial · Verde Aurora · Ouro)
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
        ctrl: (255, 114, 172),      // #FF72AC electric aurora rose (control flow)
        kw: (204, 146, 255),        // #CC92FF aurora violet (declarations)
        kw_bold: true,
        prim_ty: (64, 238, 255),    // #40EEFF glacial neon cyan
        ty: (126, 242, 255),        // #7EF2FF frost cyan
        ty_italic: false,
        func: (74, 255, 188),       // #4AFFBC electric aurora green
        mac: (168, 255, 120),       // #A8FF78 aurora lime
        std_bi: (122, 192, 255),    // #7AC0FF arctic sky
        str_col: (255, 224, 102),   // #FFE066 aurora gold
        escape: (255, 114, 156),    // #FF729C arctic rose
        num: (255, 134, 192),       // #FF86C0 aurora pink
        num_unit: (204, 146, 255),  // #CC92FF violet unit
        constant: (182, 174, 255),  // #B6AEFF polar periwinkle
        const_ident: (255, 168, 216), // #FFA8D8 frost orchid
        param: (255, 186, 116),     // #FFBA74 warm amber (italic)
        prop: (132, 216, 255),      // #84D8FF frost sky blue
        comment: (114, 136, 176),   // #7288B0 polar slate (italic)
        doc_comment: (146, 170, 212), // #92AAD4 bright ice-slate (italic)
        op: (112, 166, 255),        // #70A6FF arctic blue
        punct: (206, 218, 238),     // #CEDAEE crisp glacial silver
        var: (166, 240, 255),       // #A6F0FF pale ice
        attr: (168, 255, 120),      // #A8FF78 aurora lime
        key: (132, 216, 255),       // #84D8FF frost sky
        heading: (64, 238, 255),    // #40EEFF glacial cyan (bold)
        rainbow: [
            (255, 224, 102), // depth 0: aurora gold
            (64, 238, 255),  // depth 1: glacial cyan
            (204, 146, 255), // depth 2: aurora violet
            (74, 255, 188),  // depth 3: aurora green
        ],
        todo_bg: (64, 238, 255),
        fixme_bg: (255, 98, 126),
    },
    // 5. Nave Stealth — Electric Amethyst Jewel (Ametista · Rubi · Safira · Jade · Champanhe · Chartreuse)
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
        ctrl: (255, 86, 172),       // #FF56AC neon ruby-rose (control flow)
        kw: (206, 138, 255),        // #CE8AFF electric amethyst (declarations)
        kw_bold: true,
        prim_ty: (88, 232, 255),    // #58E8FF electric sapphire-cyan
        ty: (142, 238, 255),        // #8EEEFF diamond cyan
        ty_italic: false,
        func: (104, 255, 198),      // #68FFC6 vibrant jade mint
        mac: (182, 255, 118),       // #B6FF76 electric chartreuse
        std_bi: (164, 198, 255),    // #A4C6FF sapphire periwinkle
        str_col: (255, 220, 112),   // #FFDC70 champagne gold
        escape: (255, 86, 172),     // #FF56AC neon ruby
        num: (255, 138, 216),       // #FF8AD8 vivid orchid pink
        num_unit: (206, 138, 255),  // #CE8AFF amethyst unit
        constant: (198, 162, 255),  // #C6A2FF bright lavender
        const_ident: (255, 166, 228), // #FFA6E4 soft orchid
        param: (255, 182, 126),     // #FFB67E rose-gold peach (italic)
        prop: (164, 198, 255),      // #A4C6FF electric periwinkle
        comment: (134, 118, 166),   // #8676A6 amethyst slate (italic)
        doc_comment: (166, 148, 202), // #A694CA bright lilac-slate (italic)
        op: (255, 86, 172),         // #FF56AC neon ruby
        punct: (212, 204, 232),     // #D4CCE8 crisp lilac silver
        var: (222, 190, 255),       // #DEBEFF lavender
        attr: (182, 255, 118),      // #B6FF76 chartreuse
        key: (164, 198, 255),       // #A4C6FF periwinkle
        heading: (216, 170, 255),   // #D8AAFF amethyst (bold)
        rainbow: [
            (255, 220, 112), // depth 0: champagne gold
            (206, 138, 255), // depth 1: amethyst
            (88, 232, 255),  // depth 2: sapphire cyan
            (104, 255, 198), // depth 3: jade mint
        ],
        todo_bg: (88, 232, 255),
        fixme_bg: (255, 62, 124),
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
        ctrl: (255, 115, 198),
        kw: (255, 115, 198),
        kw_bold: false,
        prim_ty: (107, 227, 245),
        ty: (107, 227, 245),
        ty_italic: true,
        func: (92, 242, 152),
        mac: (92, 242, 152),
        std_bi: (107, 227, 245),
        str_col: (242, 229, 143),
        escape: (255, 115, 198),
        num: (199, 158, 255),
        num_unit: (255, 115, 198),
        constant: (199, 158, 255),
        const_ident: (199, 158, 255),
        param: (255, 181, 122),
        prop: (135, 230, 216),
        comment: (127, 135, 168),
        doc_comment: (146, 156, 194),
        op: (255, 115, 198),
        punct: (184, 192, 222),
        var: (199, 158, 255),
        attr: (135, 230, 216),
        key: (135, 230, 216),
        heading: (199, 158, 255),
        rainbow: [
            (242, 229, 143), // #f2e58f
            (255, 115, 198), // #ff73c6
            (107, 227, 245), // #6be3f5
            (92, 242, 152),  // #5cf298
        ],
        todo_bg: (107, 227, 245),
        fixme_bg: (255, 97, 125),
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
            TokenKind::Plain => TokenStyle { fg: self.fg, bg: None, bold: false, italic: false },
            TokenKind::ControlFlow => TokenStyle { fg: self.ctrl, bg: None, bold: self.kw_bold, italic: false },
            TokenKind::Keyword => TokenStyle { fg: self.kw, bg: None, bold: self.kw_bold, italic: false },
            TokenKind::PrimitiveType => TokenStyle { fg: self.prim_ty, bg: None, bold: false, italic: false },
            TokenKind::Type => TokenStyle { fg: self.ty, bg: None, bold: false, italic: self.ty_italic },
            TokenKind::Function => TokenStyle { fg: self.func, bg: None, bold: false, italic: false },
            TokenKind::Macro => TokenStyle { fg: self.mac, bg: None, bold: false, italic: false },
            TokenKind::StdBuiltin => TokenStyle { fg: self.std_bi, bg: None, bold: false, italic: false },
            TokenKind::String => TokenStyle { fg: self.str_col, bg: None, bold: false, italic: false },
            TokenKind::Escape => TokenStyle { fg: self.escape, bg: None, bold: false, italic: false },
            TokenKind::Number => TokenStyle { fg: self.num, bg: None, bold: false, italic: false },
            TokenKind::NumUnit => TokenStyle { fg: self.num_unit, bg: None, bold: false, italic: false },
            TokenKind::Constant => TokenStyle { fg: self.constant, bg: None, bold: false, italic: false },
            TokenKind::ConstIdent => TokenStyle { fg: self.const_ident, bg: None, bold: false, italic: false },
            TokenKind::Parameter => TokenStyle { fg: self.param, bg: None, bold: false, italic: true },
            TokenKind::Property => TokenStyle { fg: self.prop, bg: None, bold: false, italic: false },
            TokenKind::Key => TokenStyle { fg: self.key, bg: None, bold: false, italic: false },
            TokenKind::Attribute => TokenStyle { fg: self.attr, bg: None, bold: false, italic: false },
            TokenKind::Variable => TokenStyle { fg: self.var, bg: None, bold: false, italic: false },
            TokenKind::Operator => TokenStyle { fg: self.op, bg: None, bold: false, italic: false },
            TokenKind::Punctuation => TokenStyle { fg: self.punct, bg: None, bold: false, italic: false },
            TokenKind::Rainbow(d) => TokenStyle {
                fg: self.rainbow[(d as usize) % self.rainbow.len()],
                bg: None,
                bold: false,
                italic: false,
            },
            TokenKind::BracketError => TokenStyle { fg: self.fg, bg: Some(self.err), bold: true, italic: false },
            TokenKind::Comment => TokenStyle { fg: self.comment, bg: None, bold: false, italic: true },
            TokenKind::DocComment => TokenStyle { fg: self.doc_comment, bg: None, bold: false, italic: true },
            TokenKind::TodoBadge => TokenStyle { fg: self.bar_bg, bg: Some(self.todo_bg), bold: true, italic: false },
            TokenKind::FixmeBadge => TokenStyle { fg: self.bar_bg, bg: Some(self.fixme_bg), bold: true, italic: false },
            TokenKind::Heading => TokenStyle { fg: self.heading, bg: None, bold: true, italic: false },
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
                p.fg, p.kw, p.prim_ty, p.func, p.str_col, p.num, p.param, p.prop, p.comment, p.punct,
            ]
            .into_iter()
            .collect();
            assert!(
                distinct.len() >= 10,
                "Palette '{}' only has {} distinct colors across 10 core token roles!",
                p.name,
                distinct.len()
            );
            // All 5 Obsidian palettes also differentiate ControlFlow vs Keyword, Macro vs Function, and 4 unique rainbow stops
            if p.name != "dracula" {
                assert_ne!(p.ctrl, p.kw, "Palette '{}' must separate ControlFlow vs Keyword", p.name);
                assert_ne!(p.mac, p.func, "Palette '{}' must separate Macro vs Function", p.name);
                assert_ne!(p.const_ident, p.fg, "Palette '{}' must separate ConstIdent vs Plain", p.name);
            }
            let rb: HashSet<Rgb> = p.rainbow.into_iter().collect();
            assert_eq!(rb.len(), 4, "Palette '{}' must have 4 distinct rainbow stops", p.name);
        }
    }
}
