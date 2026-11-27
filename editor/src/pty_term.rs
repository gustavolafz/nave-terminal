use crate::input::{KeyCode, KeyEvent};
use crate::palette::{push_bg, push_fg_styled, Palette, Rgb};
use crate::render::char_display_width;
use std::collections::VecDeque;
use std::ffi::{CStr, CString};
use std::fmt::Write as _;
use std::io;
use std::os::unix::io::RawFd;
use std::path::Path;

const MAX_SCROLLBACK: usize = 2_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TermColor {
    Default,
    Ansi16(u8),
    Ansi256(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellStyle {
    pub fg: TermColor,
    pub bg: TermColor,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            fg: TermColor::Default,
            bg: TermColor::Default,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            inverse: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub ch: char,
    pub width: u8,
    pub style: CellStyle,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            width: 1,
            style: CellStyle::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParseState {
    Ground,
    Esc,
    EscIgnoreOne,
    Csi,
    Osc,
    OscEsc,
    StrSwallow,
    StrSwallowEsc,
}

struct AltBackup {
    grid: Vec<Vec<Cell>>,
    cur_row: usize,
    cur_col: usize,
    wrap_pending: bool,
    style: CellStyle,
    scroll_top: usize,
    scroll_bottom: usize,
}

pub struct VtScreen {
    pub cols: usize,
    pub rows: usize,
    pub grid: Vec<Vec<Cell>>,
    pub scrollback: VecDeque<Vec<Cell>>,
    pub scroll_offset: usize,
    pub cur_row: usize,
    pub cur_col: usize,
    pub wrap_pending: bool,
    pub cursor_visible: bool,
    pub app_cursor_keys: bool,
    pub bracketed_paste: bool,
    pub title: String,
    style: CellStyle,
    scroll_top: usize,
    scroll_bottom: usize,
    saved_cursor: (usize, usize, CellStyle),
    alt_backup: Option<AltBackup>,
    parse_state: ParseState,
    csi_buf: Vec<u8>,
    osc_buf: Vec<u8>,
    utf8_buf: Vec<u8>,
    pending_replies: Vec<Vec<u8>>,
}

impl VtScreen {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(2);
        let rows = rows.max(1);
        Self {
            cols,
            rows,
            grid: vec![vec![Cell::default(); cols]; rows],
            scrollback: VecDeque::with_capacity(256),
            scroll_offset: 0,
            cur_row: 0,
            cur_col: 0,
            wrap_pending: false,
            cursor_visible: true,
            app_cursor_keys: false,
            bracketed_paste: false,
            title: String::new(),
            style: CellStyle::default(),
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
            saved_cursor: (0, 0, CellStyle::default()),
            alt_backup: None,
            parse_state: ParseState::Ground,
            csi_buf: Vec::with_capacity(64),
            osc_buf: Vec::with_capacity(128),
            utf8_buf: Vec::with_capacity(4),
            pending_replies: Vec::new(),
        }
    }

    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        let new_cols = new_cols.max(2);
        let new_rows = new_rows.max(1);
        if new_cols == self.cols && new_rows == self.rows {
            return;
        }

        // Preserve top rows into scrollback if shrinking vertically above cursor
        if new_rows < self.rows && self.alt_backup.is_none() {
            let excess = self.rows - new_rows;
            let push_up = self.cur_row.saturating_sub(new_rows.saturating_sub(1)).min(excess);
            for _ in 0..push_up {
                if !self.grid.is_empty() {
                    let top = self.grid.remove(0);
                    self.push_scrollback(top);
                }
            }
            self.cur_row = self.cur_row.saturating_sub(push_up);
        }

        self.grid.resize(new_rows, vec![Cell::default(); new_cols]);
        for row in &mut self.grid {
            row.resize(new_cols, Cell::default());
        }

        self.cols = new_cols;
        self.rows = new_rows;
        self.cur_row = self.cur_row.min(new_rows.saturating_sub(1));
        self.cur_col = self.cur_col.min(new_cols.saturating_sub(1));
        self.wrap_pending = false;
        self.scroll_top = 0;
        self.scroll_bottom = new_rows.saturating_sub(1);
        self.scroll_offset = self.scroll_offset.min(self.scrollback.len());
    }

    pub fn scroll_view(&mut self, delta: isize) {
        if self.alt_backup.is_some() {
            self.scroll_offset = 0;
            return;
        }
        if delta > 0 {
            self.scroll_offset = (self.scroll_offset + delta as usize).min(self.scrollback.len());
        } else {
            self.scroll_offset = self.scroll_offset.saturating_sub((-delta) as usize);
        }
    }

    pub fn reset_scroll(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn take_replies(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.pending_replies)
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.process_byte(b);
        }
    }

    fn push_scrollback(&mut self, line: Vec<Cell>) {
        if self.scrollback.len() >= MAX_SCROLLBACK {
            self.scrollback.pop_front();
        }
        self.scrollback.push_back(line);
        if self.scroll_offset > 0 {
            self.scroll_offset = (self.scroll_offset + 1).min(self.scrollback.len());
        }
    }

    fn blank_cell(&self) -> Cell {
        Cell {
            ch: ' ',
            width: 1,
            style: CellStyle {
                bg: self.style.bg,
                ..CellStyle::default()
            },
        }
    }

    fn process_byte(&mut self, b: u8) {
        match self.parse_state {
            ParseState::Ground => self.process_ground(b),
            ParseState::Esc => self.process_esc(b),
            ParseState::EscIgnoreOne => {
                self.parse_state = ParseState::Ground;
            }
            ParseState::Csi => {
                if (0x40..=0x7e).contains(&b) {
                    let params = String::from_utf8_lossy(&self.csi_buf).into_owned();
                    self.csi_buf.clear();
                    self.parse_state = ParseState::Ground;
                    self.dispatch_csi(&params, b as char);
                } else if b == 0x1b {
                    self.csi_buf.clear();
                    self.parse_state = ParseState::Esc;
                } else if self.csi_buf.len() < 128 {
                    self.csi_buf.push(b);
                } else {
                    self.csi_buf.clear();
                    self.parse_state = ParseState::Ground;
                }
            }
            ParseState::Osc => {
                if b == 0x07 {
                    self.finish_osc();
                    self.parse_state = ParseState::Ground;
                } else if b == 0x1b {
                    self.parse_state = ParseState::OscEsc;
                } else if self.osc_buf.len() < 512 {
                    self.osc_buf.push(b);
                }
            }
            ParseState::OscEsc => {
                if b == b'\\' {
                    self.finish_osc();
                    self.parse_state = ParseState::Ground;
                } else {
                    self.osc_buf.clear();
                    self.parse_state = ParseState::Ground;
                    self.process_esc(b);
                }
            }
            ParseState::StrSwallow => {
                if b == 0x1b {
                    self.parse_state = ParseState::StrSwallowEsc;
                } else if b == 0x07 {
                    self.parse_state = ParseState::Ground;
                }
            }
            ParseState::StrSwallowEsc => {
                if b == b'\\' {
                    self.parse_state = ParseState::Ground;
                } else if b != 0x1b {
                    self.parse_state = ParseState::StrSwallow;
                }
            }
        }
    }

    fn process_ground(&mut self, b: u8) {
        if !self.utf8_buf.is_empty() {
            if (b & 0xC0) == 0x80 {
                self.utf8_buf.push(b);
                let expected = utf8_len(self.utf8_buf[0]);
                if self.utf8_buf.len() >= expected {
                    if let Ok(s) = std::str::from_utf8(&self.utf8_buf) {
                        let chs: Vec<char> = s.chars().collect();
                        self.utf8_buf.clear();
                        for ch in chs {
                            self.put_char(ch);
                        }
                    } else {
                        self.utf8_buf.clear();
                    }
                }
                return;
            } else {
                self.utf8_buf.clear();
            }
        }

        match b {
            0x1b => self.parse_state = ParseState::Esc,
            b'\r' => {
                self.cur_col = 0;
                self.wrap_pending = false;
            }
            b'\n' | 0x0b | 0x0c => {
                self.line_feed();
            }
            0x08 => {
                self.wrap_pending = false;
                if self.cur_col > 0 {
                    self.cur_col -= 1;
                }
            }
            b'\t' => {
                self.wrap_pending = false;
                let next = ((self.cur_col / 8) + 1) * 8;
                self.cur_col = next.min(self.cols.saturating_sub(1));
            }
            0x07 | 0x00 | 0x0e | 0x0f => {}
            0x20..=0x7e => self.put_char(b as char),
            0x80..=0xff => {
                let expected = utf8_len(b);
                if expected >= 2 {
                    self.utf8_buf.push(b);
                }
            }
            _ => {}
        }
    }

    fn process_esc(&mut self, b: u8) {
        match b {
            b'[' => {
                self.csi_buf.clear();
                self.parse_state = ParseState::Csi;
            }
            b']' => {
                self.osc_buf.clear();
                self.parse_state = ParseState::Osc;
            }
            b'P' | b'_' | b'^' => {
                self.parse_state = ParseState::StrSwallow;
            }
            b'(' | b')' | b'*' | b'+' | b'#' => {
                self.parse_state = ParseState::EscIgnoreOne;
            }
            b'7' => {
                self.saved_cursor = (self.cur_row, self.cur_col, self.style);
                self.parse_state = ParseState::Ground;
            }
            b'8' => {
                let (r, c, st) = self.saved_cursor;
                self.cur_row = r.min(self.rows.saturating_sub(1));
                self.cur_col = c.min(self.cols.saturating_sub(1));
                self.style = st;
                self.wrap_pending = false;
                self.parse_state = ParseState::Ground;
            }
            b'D' => {
                self.line_feed();
                self.parse_state = ParseState::Ground;
            }
            b'E' => {
                self.cur_col = 0;
                self.line_feed();
                self.parse_state = ParseState::Ground;
            }
            b'M' => {
                self.reverse_index();
                self.parse_state = ParseState::Ground;
            }
            b'c' => {
                let (c, r) = (self.cols, self.rows);
                *self = Self::new(c, r);
            }
            _ => {
                self.parse_state = ParseState::Ground;
            }
        }
    }

    fn finish_osc(&mut self) {
        let s = String::from_utf8_lossy(&self.osc_buf).into_owned();
        self.osc_buf.clear();
        if let Some((code, val)) = s.split_once(';') {
            if matches!(code, "0" | "2") {
                self.title = val.trim().to_string();
            }
        }
    }

    fn put_char(&mut self, ch: char) {
        let w = char_display_width(ch);
        if w == 0 {
            return;
        }
        if self.wrap_pending {
            self.wrap_pending = false;
            self.cur_col = 0;
            self.line_feed();
        }
        if w == 2 && self.cur_col + 1 >= self.cols {
            self.cur_col = 0;
            self.line_feed();
        }
        if self.cur_row < self.rows && self.cur_col < self.cols {
            // Clear overlapped wide char continuation if overwriting half of one
            if self.grid[self.cur_row][self.cur_col].width == 0 && self.cur_col > 0 {
                self.grid[self.cur_row][self.cur_col - 1] = Cell::default();
            }
            self.grid[self.cur_row][self.cur_col] = Cell {
                ch,
                width: w as u8,
                style: self.style,
            };
            if w == 2 && self.cur_col + 1 < self.cols {
                self.grid[self.cur_row][self.cur_col + 1] = Cell {
                    ch: ' ',
                    width: 0,
                    style: self.style,
                };
            }
        }
        if self.cur_col + w >= self.cols {
            self.cur_col = self.cols.saturating_sub(1);
            self.wrap_pending = true;
        } else {
            self.cur_col += w;
        }
    }

    fn line_feed(&mut self) {
        self.wrap_pending = false;
        if self.cur_row == self.scroll_bottom {
            self.scroll_up_region(1);
        } else if self.cur_row + 1 < self.rows {
            self.cur_row += 1;
        }
    }

    fn reverse_index(&mut self) {
        self.wrap_pending = false;
        if self.cur_row == self.scroll_top {
            self.scroll_down_region(1);
        } else if self.cur_row > 0 {
            self.cur_row -= 1;
        }
    }

    fn scroll_up_region(&mut self, n: usize) {
        let top = self.scroll_top.min(self.rows.saturating_sub(1));
        let bot = self.scroll_bottom.min(self.rows.saturating_sub(1));
        if top > bot {
            return;
        }
        let blank = vec![self.blank_cell(); self.cols];
        let count = n.min(bot - top + 1);
        for _ in 0..count {
            let removed = self.grid.remove(top);
            if top == 0 && bot == self.rows.saturating_sub(1) && self.alt_backup.is_none() {
                self.push_scrollback(removed);
            }
            self.grid.insert(bot, blank.clone());
        }
    }

    fn scroll_down_region(&mut self, n: usize) {
        let top = self.scroll_top.min(self.rows.saturating_sub(1));
        let bot = self.scroll_bottom.min(self.rows.saturating_sub(1));
        if top > bot {
            return;
        }
        let blank = vec![self.blank_cell(); self.cols];
        let count = n.min(bot - top + 1);
        for _ in 0..count {
            self.grid.remove(bot);
            self.grid.insert(top, blank.clone());
        }
    }

    fn enter_alt_screen(&mut self) {
        if self.alt_backup.is_some() {
            return;
        }
        let backup = AltBackup {
            grid: std::mem::replace(&mut self.grid, vec![vec![Cell::default(); self.cols]; self.rows]),
            cur_row: self.cur_row,
            cur_col: self.cur_col,
            wrap_pending: self.wrap_pending,
            style: self.style,
            scroll_top: self.scroll_top,
            scroll_bottom: self.scroll_bottom,
        };
        self.alt_backup = Some(backup);
        self.cur_row = 0;
        self.cur_col = 0;
        self.wrap_pending = false;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows.saturating_sub(1);
        self.scroll_offset = 0;
    }

    fn leave_alt_screen(&mut self) {
        if let Some(b) = self.alt_backup.take() {
            self.grid = b.grid;
            // Re-fit if terminal was resized while in alt screen
            self.grid.resize(self.rows, vec![Cell::default(); self.cols]);
            for row in &mut self.grid {
                row.resize(self.cols, Cell::default());
            }
            self.cur_row = b.cur_row.min(self.rows.saturating_sub(1));
            self.cur_col = b.cur_col.min(self.cols.saturating_sub(1));
            self.wrap_pending = b.wrap_pending;
            self.style = b.style;
            self.scroll_top = b.scroll_top.min(self.rows.saturating_sub(1));
            self.scroll_bottom = b.scroll_bottom.min(self.rows.saturating_sub(1));
            self.scroll_offset = 0;
        }
    }

    fn dispatch_csi(&mut self, params: &str, final_ch: char) {
        if let Some(priv_str) = params.strip_prefix('?') {
            let nums: Vec<u16> = priv_str
                .split(';')
                .filter_map(|p| p.parse::<u16>().ok())
                .collect();
            let set = final_ch == 'h';
            let reset = final_ch == 'l';
            if set || reset {
                for n in nums {
                    match n {
                        1 => self.app_cursor_keys = set,
                        25 => self.cursor_visible = set,
                        47 | 1047 | 1049 => {
                            if set {
                                self.enter_alt_screen();
                            } else {
                                self.leave_alt_screen();
                            }
                        }
                        2004 => self.bracketed_paste = set,
                        _ => {}
                    }
                }
            }
            return;
        }

        if params.starts_with('>') || params.starts_with('<') || params.starts_with('=') {
            return;
        }

        // Normalize ':' subparams (e.g. `38:2:r:g:b`) for SGR
        let normalized = params.replace(':', ";");
        let nums: Vec<usize> = if normalized.is_empty() {
            Vec::new()
        } else {
            normalized
                .split(';')
                .map(|p| p.parse::<usize>().unwrap_or(0))
                .collect()
        };
        let p0 = |def: usize| nums.first().copied().filter(|&v| v > 0).unwrap_or(def);
        let p1 = |def: usize| nums.get(1).copied().filter(|&v| v > 0).unwrap_or(def);

        match final_ch {
            'A' => {
                self.wrap_pending = false;
                self.cur_row = self.cur_row.saturating_sub(p0(1));
            }
            'B' | 'e' => {
                self.wrap_pending = false;
                self.cur_row = (self.cur_row + p0(1)).min(self.rows.saturating_sub(1));
            }
            'C' | 'a' => {
                self.wrap_pending = false;
                self.cur_col = (self.cur_col + p0(1)).min(self.cols.saturating_sub(1));
            }
            'D' => {
                self.wrap_pending = false;
                self.cur_col = self.cur_col.saturating_sub(p0(1));
            }
            'E' => {
                self.wrap_pending = false;
                self.cur_row = (self.cur_row + p0(1)).min(self.rows.saturating_sub(1));
                self.cur_col = 0;
            }
            'F' => {
                self.wrap_pending = false;
                self.cur_row = self.cur_row.saturating_sub(p0(1));
                self.cur_col = 0;
            }
            'G' | '`' => {
                self.wrap_pending = false;
                self.cur_col = p0(1).saturating_sub(1).min(self.cols.saturating_sub(1));
            }
            'd' => {
                self.wrap_pending = false;
                self.cur_row = p0(1).saturating_sub(1).min(self.rows.saturating_sub(1));
            }
            'H' | 'f' => {
                self.wrap_pending = false;
                let r = p0(1).saturating_sub(1).min(self.rows.saturating_sub(1));
                let c = p1(1).saturating_sub(1).min(self.cols.saturating_sub(1));
                self.cur_row = r;
                self.cur_col = c;
            }
            'J' => {
                let mode = nums.first().copied().unwrap_or(0);
                let blank = self.blank_cell();
                match mode {
                    0 => {
                        for c in self.cur_col..self.cols {
                            self.grid[self.cur_row][c] = blank;
                        }
                        for r in (self.cur_row + 1)..self.rows {
                            self.grid[r].fill(blank);
                        }
                    }
                    1 => {
                        for r in 0..self.cur_row {
                            self.grid[r].fill(blank);
                        }
                        for c in 0..=self.cur_col.min(self.cols.saturating_sub(1)) {
                            self.grid[self.cur_row][c] = blank;
                        }
                    }
                    2 => {
                        for r in 0..self.rows {
                            self.grid[r].fill(blank);
                        }
                    }
                    3 => {
                        self.scrollback.clear();
                        self.scroll_offset = 0;
                    }
                    _ => {}
                }
            }
            'K' => {
                let mode = nums.first().copied().unwrap_or(0);
                let blank = self.blank_cell();
                match mode {
                    0 => {
                        for c in self.cur_col..self.cols {
                            self.grid[self.cur_row][c] = blank;
                        }
                    }
                    1 => {
                        for c in 0..=self.cur_col.min(self.cols.saturating_sub(1)) {
                            self.grid[self.cur_row][c] = blank;
                        }
                    }
                    2 => {
                        self.grid[self.cur_row].fill(blank);
                    }
                    _ => {}
                }
            }
            'X' => {
                let n = p0(1);
                let blank = self.blank_cell();
                let end = (self.cur_col + n).min(self.cols);
                for c in self.cur_col..end {
                    self.grid[self.cur_row][c] = blank;
                }
            }
            'L' => {
                if self.cur_row >= self.scroll_top && self.cur_row <= self.scroll_bottom {
                    let n = p0(1).min(self.scroll_bottom - self.cur_row + 1);
                    let blank = vec![self.blank_cell(); self.cols];
                    for _ in 0..n {
                        self.grid.remove(self.scroll_bottom);
                        self.grid.insert(self.cur_row, blank.clone());
                    }
                }
            }
            'M' => {
                if self.cur_row >= self.scroll_top && self.cur_row <= self.scroll_bottom {
                    let n = p0(1).min(self.scroll_bottom - self.cur_row + 1);
                    let blank = vec![self.blank_cell(); self.cols];
                    for _ in 0..n {
                        self.grid.remove(self.cur_row);
                        self.grid.insert(self.scroll_bottom, blank.clone());
                    }
                }
            }
            '@' => {
                let n = p0(1).min(self.cols.saturating_sub(self.cur_col));
                let blank = self.blank_cell();
                let row = &mut self.grid[self.cur_row];
                for _ in 0..n {
                    row.pop();
                    row.insert(self.cur_col, blank);
                }
            }
            'P' => {
                let n = p0(1).min(self.cols.saturating_sub(self.cur_col));
                let blank = self.blank_cell();
                let row = &mut self.grid[self.cur_row];
                for _ in 0..n {
                    row.remove(self.cur_col);
                    row.push(blank);
                }
            }
            'S' => self.scroll_up_region(p0(1)),
            'T' => self.scroll_down_region(p0(1)),
            'r' => {
                let top = p0(1).saturating_sub(1).min(self.rows.saturating_sub(1));
                let bot = p1(self.rows).saturating_sub(1).min(self.rows.saturating_sub(1));
                if top < bot {
                    self.scroll_top = top;
                    self.scroll_bottom = bot;
                } else {
                    self.scroll_top = 0;
                    self.scroll_bottom = self.rows.saturating_sub(1);
                }
                self.cur_row = 0;
                self.cur_col = 0;
                self.wrap_pending = false;
            }
            's' => {
                self.saved_cursor = (self.cur_row, self.cur_col, self.style);
            }
            'u' => {
                let (r, c, st) = self.saved_cursor;
                self.cur_row = r.min(self.rows.saturating_sub(1));
                self.cur_col = c.min(self.cols.saturating_sub(1));
                self.style = st;
                self.wrap_pending = false;
            }
            'n' => {
                if nums.first().copied() == Some(6) {
                    let reply = format!("\x1b[{};{}R", self.cur_row + 1, self.cur_col + 1);
                    self.pending_replies.push(reply.into_bytes());
                }
            }
            'm' => self.apply_sgr(&nums),
            _ => {}
        }
    }

    fn apply_sgr(&mut self, nums: &[usize]) {
        if nums.is_empty() {
            self.style = CellStyle::default();
            return;
        }
        let mut i = 0;
        while i < nums.len() {
            match nums[i] {
                0 => self.style = CellStyle::default(),
                1 => self.style.bold = true,
                2 => self.style.dim = true,
                3 => self.style.italic = true,
                4 => self.style.underline = true,
                7 => self.style.inverse = true,
                22 => {
                    self.style.bold = false;
                    self.style.dim = false;
                }
                23 => self.style.italic = false,
                24 => self.style.underline = false,
                27 => self.style.inverse = false,
                30..=37 => self.style.fg = TermColor::Ansi16((nums[i] - 30) as u8),
                39 => self.style.fg = TermColor::Default,
                40..=47 => self.style.bg = TermColor::Ansi16((nums[i] - 40) as u8),
                49 => self.style.bg = TermColor::Default,
                90..=97 => self.style.fg = TermColor::Ansi16((nums[i] - 90 + 8) as u8),
                100..=107 => self.style.bg = TermColor::Ansi16((nums[i] - 100 + 8) as u8),
                38 => {
                    if i + 2 < nums.len() && nums[i + 1] == 5 {
                        self.style.fg = TermColor::Ansi256(nums[i + 2].min(255) as u8);
                        i += 2;
                    } else if i + 4 < nums.len() && nums[i + 1] == 2 {
                        self.style.fg = TermColor::Rgb(
                            nums[i + 2].min(255) as u8,
                            nums[i + 3].min(255) as u8,
                            nums[i + 4].min(255) as u8,
                        );
                        i += 4;
                    }
                }
                48 => {
                    if i + 2 < nums.len() && nums[i + 1] == 5 {
                        self.style.bg = TermColor::Ansi256(nums[i + 2].min(255) as u8);
                        i += 2;
                    } else if i + 4 < nums.len() && nums[i + 1] == 2 {
                        self.style.bg = TermColor::Rgb(
                            nums[i + 2].min(255) as u8,
                            nums[i + 3].min(255) as u8,
                            nums[i + 4].min(255) as u8,
                        );
                        i += 4;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    pub fn visible_row(&self, vis_y: usize) -> Option<&[Cell]> {
        if self.alt_backup.is_some() || self.scroll_offset == 0 {
            self.grid.get(vis_y).map(|v| v.as_slice())
        } else {
            let sb_len = self.scrollback.len();
            let start = sb_len.saturating_sub(self.scroll_offset);
            let idx = start + vis_y;
            if idx < sb_len {
                self.scrollback.get(idx).map(|v| v.as_slice())
            } else {
                self.grid.get(idx - sb_len).map(|v| v.as_slice())
            }
        }
    }
}

pub struct IntegratedTerminal {
    master_fd: RawFd,
    child_pid: libc::pid_t,
    pub screen: VtScreen,
    pub cwd_label: String,
    pub shell_name: String,
    pub alive: bool,
}

impl IntegratedTerminal {
    pub fn spawn(cwd: &Path, cols: usize, rows: usize) -> io::Result<Self> {
        let cols = cols.max(10);
        let rows = rows.max(2);

        let master_fd = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
        if master_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        unsafe {
            if libc::grantpt(master_fd) != 0 || libc::unlockpt(master_fd) != 0 {
                let err = io::Error::last_os_error();
                libc::close(master_fd);
                return Err(err);
            }
        }

        let slave_name = unsafe {
            let ptr = libc::ptsname(master_fd);
            if ptr.is_null() {
                let err = io::Error::last_os_error();
                libc::close(master_fd);
                return Err(err);
            }
            CStr::from_ptr(ptr).to_owned()
        };

        let slave_fd = unsafe { libc::open(slave_name.as_ptr(), libc::O_RDWR | libc::O_NOCTTY) };
        if slave_fd < 0 {
            let err = io::Error::last_os_error();
            unsafe { libc::close(master_fd) };
            return Err(err);
        }

        let ws = libc::winsize {
            ws_row: rows as u16,
            ws_col: cols as u16,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        unsafe {
            libc::ioctl(slave_fd, libc::TIOCSWINSZ, &ws);
        }

        let shell_path = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty() && Path::new(s).exists())
            .or_else(|| {
                ["/bin/zsh", "/usr/bin/zsh", "/bin/bash", "/bin/sh"]
                    .iter()
                    .find(|p| Path::new(p).exists())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "/bin/sh".to_string());

        let shell_name = Path::new(&shell_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("sh")
            .to_string();

        let cwd_str = cwd.to_string_lossy().into_owned();
        let c_shell = CString::new(shell_path.as_str()).unwrap_or_else(|_| CString::new("/bin/sh").unwrap());
        let c_cwd = CString::new(cwd_str.as_str()).ok();

        let pid = unsafe { libc::fork() };
        if pid < 0 {
            let err = io::Error::last_os_error();
            unsafe {
                libc::close(slave_fd);
                libc::close(master_fd);
            }
            return Err(err);
        }

        if pid == 0 {
            // Child process
            unsafe {
                libc::close(master_fd);
                libc::setsid();
                libc::ioctl(slave_fd, libc::TIOCSCTTY as _, 0);
                libc::dup2(slave_fd, libc::STDIN_FILENO);
                libc::dup2(slave_fd, libc::STDOUT_FILENO);
                libc::dup2(slave_fd, libc::STDERR_FILENO);
                if slave_fd > 2 {
                    libc::close(slave_fd);
                }
                for fd in 3..64 {
                    libc::close(fd);
                }
                if let Some(ref dir_c) = c_cwd {
                    libc::chdir(dir_c.as_ptr());
                }
                std::env::set_var("TERM", "xterm-256color");
                std::env::set_var("COLORTERM", "truecolor");
                std::env::set_var("NAVE_INTEGRATED", "1");
                let argv = [c_shell.as_ptr(), std::ptr::null()];
                libc::execvp(c_shell.as_ptr(), argv.as_ptr());
                libc::_exit(127);
            }
        }

        // Parent process
        unsafe {
            libc::close(slave_fd);
            let flags = libc::fcntl(master_fd, libc::F_GETFL);
            if flags >= 0 {
                libc::fcntl(master_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
            let fd_flags = libc::fcntl(master_fd, libc::F_GETFD);
            if fd_flags >= 0 {
                libc::fcntl(master_fd, libc::F_SETFD, fd_flags | libc::FD_CLOEXEC);
            }
        }

        let cwd_label = compact_cwd(cwd);
        Ok(Self {
            master_fd,
            child_pid: pid,
            screen: VtScreen::new(cols, rows),
            cwd_label,
            shell_name,
            alive: true,
        })
    }

    pub fn master_fd(&self) -> RawFd {
        self.master_fd
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(10);
        let rows = rows.max(2);
        if cols == self.screen.cols && rows == self.screen.rows {
            return;
        }
        self.screen.resize(cols, rows);
        let ws = libc::winsize {
            ws_row: rows as u16,
            ws_col: cols as u16,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        unsafe {
            libc::ioctl(self.master_fd, libc::TIOCSWINSZ, &ws);
        }
    }

    pub fn write_bytes(&mut self, data: &[u8]) {
        if !self.alive || data.is_empty() {
            return;
        }
        self.screen.reset_scroll();
        let mut off = 0;
        while off < data.len() {
            let n = unsafe {
                libc::write(
                    self.master_fd,
                    data[off..].as_ptr() as *const libc::c_void,
                    data.len() - off,
                )
            };
            if n > 0 {
                off += n as usize;
            } else {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::WouldBlock {
                    // Brief yield if PTY buffer temporarily full
                    std::thread::sleep(std::time::Duration::from_millis(2));
                    continue;
                }
                break;
            }
        }
    }

    pub fn send_key(&mut self, k: KeyEvent) {
        let bytes = encode_key_for_pty(k, self.screen.app_cursor_keys);
        if !bytes.is_empty() {
            self.write_bytes(&bytes);
        }
    }

    pub fn send_paste(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let cr_text = text.replace('\n', "\r");
        if self.screen.bracketed_paste {
            let mut buf = Vec::with_capacity(cr_text.len() + 12);
            buf.extend_from_slice(b"\x1b[200~");
            buf.extend_from_slice(cr_text.as_bytes());
            buf.extend_from_slice(b"\x1b[201~");
            self.write_bytes(&buf);
        } else {
            self.write_bytes(cr_text.as_bytes());
        }
    }

    /// Reads all currently available PTY output into the VT screen.
    /// Returns `true` if screen state changed or shell exited.
    pub fn drain_output(&mut self) -> bool {
        if !self.alive {
            return false;
        }
        let mut changed = false;
        let mut buf = [0u8; 8192];
        for _ in 0..32 {
            let n = unsafe {
                libc::read(
                    self.master_fd,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                )
            };
            if n > 0 {
                self.screen.feed(&buf[..n as usize]);
                changed = true;
            } else if n == 0 {
                self.alive = false;
                changed = true;
                break;
            } else {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::WouldBlock || err.kind() == io::ErrorKind::Interrupted {
                    break;
                }
                // EIO on Linux PTY master means slave side closed (shell exited)
                self.alive = false;
                changed = true;
                break;
            }
        }

        // Send any pending terminal replies (e.g. DSR 6n cursor position query)
        for reply in self.screen.take_replies() {
            self.write_bytes(&reply);
        }

        // Non-blocking reap check
        if self.alive {
            let mut status: libc::c_int = 0;
            let r = unsafe { libc::waitpid(self.child_pid, &mut status, libc::WNOHANG) };
            if r > 0 {
                self.alive = false;
                changed = true;
            }
        }
        changed
    }
}

impl Drop for IntegratedTerminal {
    fn drop(&mut self) {
        unsafe {
            if self.child_pid > 0 {
                libc::kill(-self.child_pid, libc::SIGHUP);
                libc::kill(self.child_pid, libc::SIGHUP);
                let mut status: libc::c_int = 0;
                for _ in 0..5 {
                    if libc::waitpid(self.child_pid, &mut status, libc::WNOHANG) != 0 {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
            if self.master_fd >= 0 {
                libc::close(self.master_fd);
            }
        }
    }
}

pub fn encode_key_for_pty(k: KeyEvent, app_cursor: bool) -> Vec<u8> {
    let xterm_mod = || -> u8 {
        1 + if k.shift { 1 } else { 0 } + if k.alt { 2 } else { 0 } + if k.ctrl { 4 } else { 0 }
    };
    let has_mod = k.ctrl || k.alt || k.shift;

    match k.code {
        KeyCode::Char(c) => {
            if k.ctrl && !k.alt {
                let lc = c.to_ascii_lowercase();
                let byte = match lc {
                    'a'..='z' => Some((lc as u8) - b'a' + 1),
                    ' ' | '@' => Some(0x00),
                    '[' => Some(0x1b),
                    '\\' => Some(0x1c),
                    ']' => Some(0x1d),
                    '^' => Some(0x1e),
                    '_' | '/' => Some(0x1f),
                    _ => None,
                };
                if let Some(b) = byte {
                    return vec![b];
                }
            }
            let out_ch = if k.alt && k.shift {
                c.to_ascii_uppercase()
            } else {
                c
            };
            let mut s = [0u8; 4];
            let bytes = out_ch.encode_utf8(&mut s).as_bytes();
            if k.alt {
                let mut v = Vec::with_capacity(1 + bytes.len());
                v.push(0x1b);
                v.extend_from_slice(bytes);
                v
            } else {
                bytes.to_vec()
            }
        }
        KeyCode::Enter => {
            if k.alt {
                b"\x1b\r".to_vec()
            } else {
                b"\r".to_vec()
            }
        }
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => {
            if k.alt {
                b"\x1b\x7f".to_vec()
            } else if k.ctrl {
                b"\x17".to_vec() // Ctrl+W word delete in zsh/bash
            } else {
                b"\x7f".to_vec()
            }
        }
        KeyCode::Esc => b"\x1b".to_vec(),
        KeyCode::Up => encode_arrow(b'A', has_mod, xterm_mod(), app_cursor),
        KeyCode::Down => encode_arrow(b'B', has_mod, xterm_mod(), app_cursor),
        KeyCode::Right => encode_arrow(b'C', has_mod, xterm_mod(), app_cursor),
        KeyCode::Left => encode_arrow(b'D', has_mod, xterm_mod(), app_cursor),
        KeyCode::Home => encode_arrow(b'H', has_mod, xterm_mod(), app_cursor),
        KeyCode::End => encode_arrow(b'F', has_mod, xterm_mod(), app_cursor),
        KeyCode::Insert => encode_tilde(2, has_mod, xterm_mod()),
        KeyCode::Delete => encode_tilde(3, has_mod, xterm_mod()),
        KeyCode::PageUp => encode_tilde(5, has_mod, xterm_mod()),
        KeyCode::PageDown => encode_tilde(6, has_mod, xterm_mod()),
        KeyCode::F(n) => match n {
            1 => b"\x1bOP".to_vec(),
            2 => b"\x1bOQ".to_vec(),
            3 => b"\x1bOR".to_vec(),
            4 => b"\x1bOS".to_vec(),
            5 => b"\x1b[15~".to_vec(),
            6 => b"\x1b[17~".to_vec(),
            7 => b"\x1b[18~".to_vec(),
            8 => b"\x1b[19~".to_vec(),
            9 => b"\x1b[20~".to_vec(),
            10 => b"\x1b[21~".to_vec(),
            11 => b"\x1b[23~".to_vec(),
            12 => b"\x1b[24~".to_vec(),
            _ => Vec::new(),
        },
    }
}

fn encode_arrow(ch: u8, has_mod: bool, m: u8, app_cursor: bool) -> Vec<u8> {
    if has_mod {
        format!("\x1b[1;{}{}", m, ch as char).into_bytes()
    } else if app_cursor {
        vec![0x1b, b'O', ch]
    } else {
        vec![0x1b, b'[', ch]
    }
}

fn encode_tilde(code: u8, has_mod: bool, m: u8) -> Vec<u8> {
    if has_mod {
        format!("\x1b[{};{}~", code, m).into_bytes()
    } else {
        format!("\x1b[{}~", code).into_bytes()
    }
}

fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if (first & 0xE0) == 0xC0 {
        2
    } else if (first & 0xF0) == 0xE0 {
        3
    } else if (first & 0xF8) == 0xF0 {
        4
    } else {
        1
    }
}

fn compact_cwd(dir: &Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let hp = Path::new(&home);
        if dir == hp {
            return "~".to_string();
        }
        if let Ok(rel) = dir.strip_prefix(hp) {
            return format!("~/{}", rel.to_string_lossy());
        }
    }
    dir.to_string_lossy().into_owned()
}

pub fn resolve_term_color(c: TermColor, pal: &Palette) -> Option<Rgb> {
    match c {
        TermColor::Default => None,
        TermColor::Rgb(r, g, b) => Some((r, g, b)),
        TermColor::Ansi16(idx) => Some(ansi16_rgb(idx, pal)),
        TermColor::Ansi256(idx) => Some(ansi256_rgb(idx, pal)),
    }
}

fn ansi16_rgb(idx: u8, pal: &Palette) -> Rgb {
    match idx {
        0 => (22, 25, 42),
        1 => pal.err,
        2 => pal.ok,
        3 => pal.warn,
        4 => pal.std_bi,
        5 => pal.kw,
        6 => pal.grad[0],
        7 => pal.fg,
        8 => pal.dim,
        9 => (255, 92, 142),
        10 => (96, 255, 196),
        11 => (255, 240, 112),
        12 => (130, 224, 255),
        13 => (230, 176, 255),
        14 => (116, 252, 255),
        _ => (255, 255, 255),
    }
}

fn ansi256_rgb(idx: u8, pal: &Palette) -> Rgb {
    if idx < 16 {
        return ansi16_rgb(idx, pal);
    }
    if idx >= 232 {
        let v = 8 + (idx - 232) * 10;
        return (v, v, v);
    }
    let c = idx - 16;
    let r = c / 36;
    let g = (c % 36) / 6;
    let b = c % 6;
    let map = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
    (map(r), map(g), map(b))
}

/// Renders the integrated terminal rows inside `[y_start .. y_start + height]` × `[0 .. cols]`.
/// Returns the hardware cursor screen coordinates `(abs_row, abs_col)` when focused & live.
pub fn render_terminal_cells(
    out: &mut String,
    screen: &VtScreen,
    y_start: usize,
    cols: usize,
    height: usize,
    is_focused: bool,
    pal: &Palette,
    default_bg: Option<Rgb>,
) -> Option<(usize, usize)> {
    for vy in 0..height {
        let abs_row = y_start + vy;
        let _ = write!(out, "\x1b[{};1H", abs_row + 1);
        let mut last_fg: Option<Rgb> = None;
        let mut last_bg: Option<Rgb> = None;
        let mut last_bold = false;
        let mut last_italic = false;
        let mut last_underline = false;
        let mut style_init = false;

        let row_cells = screen.visible_row(vy);
        let mut drawn = 0usize;

        while drawn < cols {
            let cell = row_cells
                .and_then(|r| r.get(drawn))
                .copied()
                .unwrap_or_default();

            if cell.width == 0 {
                drawn += 1;
                continue;
            }
            let cw = (cell.width as usize).max(1);
            if drawn + cw > cols {
                if let Some(bg_rgb) = default_bg {
                    push_bg(out, bg_rgb);
                } else {
                    out.push_str("\x1b[49m");
                }
                out.push(' ');
                drawn += 1;
                continue;
            }

            let mut fg = resolve_term_color(cell.style.fg, pal);
            let mut bg = resolve_term_color(cell.style.bg, pal);
            if cell.style.inverse {
                let eff_fg = fg.unwrap_or(pal.fg);
                let eff_bg = bg.unwrap_or(pal.bar_bg);
                fg = Some(eff_bg);
                bg = Some(eff_fg);
            } else if cell.style.dim {
                fg = Some(pal.dim);
            }

            let fg_rgb = fg.unwrap_or(pal.fg);
            let need_style = !style_init
                || Some(fg_rgb) != last_fg
                || bg != last_bg
                || cell.style.bold != last_bold
                || cell.style.italic != last_italic
                || cell.style.underline != last_underline;

            if need_style {
                if let Some(bg_rgb) = bg.or(default_bg) {
                    push_bg(out, bg_rgb);
                } else {
                    out.push_str("\x1b[49m");
                }
                push_fg_styled(out, fg_rgb, cell.style.bold, cell.style.italic);
                if cell.style.underline {
                    out.push_str("\x1b[4m");
                } else if last_underline {
                    out.push_str("\x1b[24m");
                }
                last_fg = Some(fg_rgb);
                last_bg = bg;
                last_bold = cell.style.bold;
                last_italic = cell.style.italic;
                last_underline = cell.style.underline;
                style_init = true;
            }

            out.push(cell.ch);
            drawn += cw;
        }
        out.push_str("\x1b[0m");
    }

    if is_focused && screen.scroll_offset == 0 && screen.cursor_visible && screen.cur_row < height {
        Some((
            y_start + screen.cur_row,
            screen.cur_col.min(cols.saturating_sub(1)),
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vt_screen_parses_sgr_cursor_and_scrollback() {
        let mut vt = VtScreen::new(20, 3);
        vt.feed(b"\x1b[38;2;0;248;255mhello\x1b[0m\r\nline2\r\nline3\r\nline4");
        assert_eq!(vt.scrollback.len(), 1);
        let row0: String = vt.grid[0].iter().take(5).map(|c| c.ch).collect();
        let row2: String = vt.grid[2].iter().take(5).map(|c| c.ch).collect();
        assert_eq!(row0, "line2");
        assert_eq!(row2, "line4");
        assert_eq!(vt.scrollback[0][0].style.fg, TermColor::Rgb(0, 248, 255));

        // DSR 6n generates cursor reply
        vt.feed(b"\x1b[6n");
        let replies = vt.take_replies();
        assert_eq!(replies.len(), 1);
        assert_eq!(String::from_utf8_lossy(&replies[0]), "\x1b[3;6R");
    }

    #[test]
    fn vt_screen_alt_screen_restores_primary_grid() {
        let mut vt = VtScreen::new(16, 4);
        vt.feed(b"primary");
        vt.feed(b"\x1b[?1049hALT_CONTENT");
        let alt_txt: String = vt.grid[0].iter().take(11).map(|c| c.ch).collect();
        assert_eq!(alt_txt, "ALT_CONTENT");
        vt.feed(b"\x1b[?1049l");
        let pri_txt: String = vt.grid[0].iter().take(7).map(|c| c.ch).collect();
        assert_eq!(pri_txt, "primary");
    }

    #[test]
    fn integrated_pty_spawns_and_runs_command() {
        let cwd = std::env::temp_dir();
        let mut term = IntegratedTerminal::spawn(&cwd, 80, 10).expect("pty spawn");
        term.write_bytes(b"echo NAVE_PTY_OK; exit\r");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        let mut found = false;
        while std::time::Instant::now() < deadline {
            term.drain_output();
            for row in &term.screen.grid {
                let s: String = row.iter().map(|c| c.ch).collect();
                if s.contains("NAVE_PTY_OK") {
                    found = true;
                }
            }
            if found && !term.alive {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(found, "expected NAVE_PTY_OK in PTY output");
    }
}
