mod config;
mod document;
mod git;
mod input;
mod palette;
mod picker;
mod render;
mod rope;
mod search;
mod syntax;
mod terminal;

use config::EditorConfig;
use document::{Document, Motion};
use input::{Event, InputDecoder, KeyCode, KeyEvent, MouseKind};
use palette::Palette;
use picker::{Picker, PickerAction};
use render::{char_col_at_visual, ensure_cursor_visible, render_editor, LayoutInfo, PromptBar};
use rope::LineEnding;
use std::env;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use terminal::{copy_to_system_clipboard, paste_from_system_clipboard, Terminal};

const VERSION: &str = "nave-edit 0.1.0 (Rust Native · Nave Terminal v4)";

enum Mode {
    Edit,
    Find,
    Replace { focus_replace: bool },
    GotoLine(String),
    SaveAs(String),
    ConfirmQuit,
    ConfirmCloseTab,
    Picker(Picker),
}

struct App {
    tabs: Vec<Document>,
    active_tab: usize,
    palette: &'static Palette,
    cfg: EditorConfig,
    term_sync_needed: bool,
    mode: Mode,
    clipboard: String,
    status_msg: Option<(String, Instant)>,
    layout: LayoutInfo,
    dragging: bool,
    last_click: Option<(Instant, usize, usize, u8)>,
    should_quit: bool,
    last_mtime_check: Instant,
}

impl App {
    fn new(mut tabs: Vec<Document>, palette: &'static Palette, cfg: EditorConfig) -> Self {
        if tabs.is_empty() {
            tabs.push(Document::new_empty());
        }
        for t in &mut tabs {
            t.auto_close = cfg.auto_close;
        }
        Self {
            tabs,
            active_tab: 0,
            palette,
            cfg,
            term_sync_needed: false,
            mode: Mode::Edit,
            clipboard: String::new(),
            status_msg: None,
            layout: LayoutInfo::default(),
            dragging: false,
            last_click: None,
            should_quit: false,
            last_mtime_check: Instant::now(),
        }
    }

    fn doc(&self) -> &Document {
        &self.tabs[self.active_tab]
    }

    fn doc_mut(&mut self) -> &mut Document {
        &mut self.tabs[self.active_tab]
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_msg = Some((msg.into(), Instant::now()));
    }

    fn active_status(&self) -> Option<&str> {
        self.status_msg.as_ref().and_then(|(s, t)| {
            if t.elapsed() < Duration::from_secs(4) {
                Some(s.as_str())
            } else {
                None
            }
        })
    }

    fn base_dir(&self) -> PathBuf {
        self.doc()
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    fn open_or_focus_file(&mut self, path: &Path, jump_line: Option<usize>) {
        let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if let Some(idx) = self.tabs.iter().position(|t| {
            t.path
                .as_ref()
                .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()) == canon)
                .unwrap_or(false)
        }) {
            self.active_tab = idx;
            if let Some(ln) = jump_line {
                self.doc_mut().goto_line(ln);
            }
            return;
        }
        match Document::open(path, jump_line) {
            Ok(doc) => {
                // Replace initial untouched blank tab if present
                if self.tabs.len() == 1
                    && self.tabs[0].path.is_none()
                    && !self.tabs[0].is_dirty()
                    && self.tabs[0].rope.is_empty()
                {
                    self.tabs[0] = doc;
                    self.active_tab = 0;
                } else {
                    self.tabs.push(doc);
                    self.active_tab = self.tabs.len() - 1;
                }
                let name = self.doc().display_name();
                self.set_status(format!("Aberto: {}", name));
            }
            Err(e) => self.set_status(format!("Erro ao abrir: {}", e)),
        }
    }

    fn handle_event(&mut self, ev: Event, cols: usize, rows: usize) {
        match ev {
            Event::Paste(text) => self.handle_paste(&text),
            Event::Mouse(m) => self.handle_mouse(m, cols, rows),
            Event::Key(k) => self.handle_key(k, cols, rows),
        }
    }

    fn handle_paste(&mut self, text: &str) {
        match &mut self.mode {
            Mode::Edit => self.doc_mut().paste(text),
            Mode::Find => {
                let clean = text.replace('\n', " ");
                self.doc_mut().search.query.push_str(&clean);
                self.recompute_search_and_jump();
            }
            Mode::Replace { focus_replace } => {
                let clean = text.replace('\n', " ");
                if *focus_replace {
                    self.doc_mut().search.replace.push_str(&clean);
                } else {
                    self.doc_mut().search.query.push_str(&clean);
                    self.recompute_search_and_jump();
                }
            }
            Mode::GotoLine(buf) | Mode::SaveAs(buf) => buf.push_str(text.trim()),
            Mode::Picker(pk) => {
                pk.query.push_str(&text.replace('\n', ""));
                pk.refilter();
            }
            Mode::ConfirmQuit | Mode::ConfirmCloseTab => {}
        }
    }

    fn handle_mouse(&mut self, m: input::MouseEvent, _cols: usize, rows: usize) {
        if matches!(self.mode, Mode::Picker(_)) {
            if let Mode::Picker(ref mut pk) = self.mode {
                match m.kind {
                    MouseKind::WheelUp => pk.move_sel(-1),
                    MouseKind::WheelDown => pk.move_sel(1),
                    _ => {}
                }
            }
            return;
        }

        match m.kind {
            MouseKind::WheelUp => {
                self.doc_mut().scroll_row = self.doc().scroll_row.saturating_sub(3);
            }
            MouseKind::WheelDown => {
                let max_r = self.doc().rope.line_count().saturating_sub(1);
                self.doc_mut().scroll_row = (self.doc().scroll_row + 3).min(max_r);
            }
            MouseKind::LeftDown => {
                // Clicked tab bar (row 0)?
                if m.row == 0 {
                    for &(sc, ec, tidx) in &self.layout.tab_hitboxes {
                        if m.col >= sc && m.col < ec && tidx < self.tabs.len() {
                            self.active_tab = tidx;
                            self.mode = Mode::Edit;
                            return;
                        }
                    }
                    return;
                }
                if let Some((doc_row, doc_col)) = self.screen_to_doc(m.col, m.row, rows) {
                    let now = Instant::now();
                    let clicks = if let Some((prev_t, pr, pc, cnt)) = self.last_click {
                        if now.duration_since(prev_t).as_millis() < 400 && pr == m.row && pc.abs_diff(m.col) <= 1 {
                            (cnt % 3) + 1
                        } else {
                            1
                        }
                    } else {
                        1
                    };
                    self.last_click = Some((now, m.row, m.col, clicks));
                    self.mode = Mode::Edit;

                    match clicks {
                        2 => {
                            self.doc_mut().select_word_at_line_col(doc_row, doc_col);
                            self.dragging = false;
                        }
                        3 => {
                            self.doc_mut().select_full_line_at(doc_row);
                            self.dragging = false;
                        }
                        _ => {
                            self.doc_mut().set_cursor_at_line_col(doc_row, doc_col, m.shift);
                            self.dragging = true;
                        }
                    }
                }
            }
            MouseKind::LeftDrag => {
                if self.dragging {
                    if let Some((doc_row, doc_col)) = self.screen_to_doc(m.col, m.row, rows) {
                        self.doc_mut().set_cursor_at_line_col(doc_row, doc_col, true);
                    }
                }
            }
            MouseKind::LeftUp => {
                self.dragging = false;
            }
        }
    }

    fn screen_to_doc(&self, col: usize, row: usize, _rows: usize) -> Option<(usize, usize)> {
        if row < self.layout.content_start_row {
            return None;
        }
        let rel_y = row - self.layout.content_start_row;
        let vinfo = self.layout.visual_rows.get(rel_y)?;
        let line_txt = self.doc().rope.line(vinfo.doc_row);
        let rel_x = col.saturating_sub(self.layout.gutter_width);

        if self.cfg.wrap {
            let seg: String = line_txt
                .chars()
                .skip(vinfo.start_char_col)
                .take(vinfo.end_char_col - vinfo.start_char_col)
                .collect();
            let c_in_seg = char_col_at_visual(&seg, rel_x, self.doc().tab_width);
            Some((vinfo.doc_row, vinfo.start_char_col + c_in_seg))
        } else {
            let target_vis = self.doc().scroll_col + rel_x;
            let c = char_col_at_visual(&line_txt, target_vis, self.doc().tab_width);
            Some((vinfo.doc_row, c))
        }
    }

    fn handle_key(&mut self, k: KeyEvent, _cols: usize, rows: usize) {
        // Global Alt+1..5 live palette switch
        if k.alt && !k.ctrl {
            if let KeyCode::Char(c @ '1'..='5') = k.code {
                let idx = (c as u8 - b'1') as usize;
                self.palette = Palette::by_index(idx);
                self.set_status(format!("Paleta: {}", self.palette.name));
                return;
            }
        }

        match &mut self.mode {
            Mode::Picker(_) => {
                self.handle_picker_key(k);
                return;
            }
            Mode::ConfirmQuit => {
                match k.code {
                    KeyCode::Char('s') | KeyCode::Char('S') | KeyCode::Char('y') | KeyCode::Char('Y') => {
                        if self.save_all_dirty() {
                            self.should_quit = true;
                        } else {
                            self.mode = Mode::Edit;
                        }
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') => {
                        self.should_quit = true;
                    }
                    KeyCode::Esc | KeyCode::Char('c') => {
                        self.mode = Mode::Edit;
                    }
                    _ => {}
                }
                return;
            }
            Mode::ConfirmCloseTab => {
                match k.code {
                    KeyCode::Char('s') | KeyCode::Char('S') | KeyCode::Char('y') | KeyCode::Char('Y') => {
                        if self.try_save_active() {
                            self.close_active_tab_force();
                        }
                        self.mode = Mode::Edit;
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') => {
                        self.close_active_tab_force();
                        self.mode = Mode::Edit;
                    }
                    KeyCode::Esc => {
                        self.mode = Mode::Edit;
                    }
                    _ => {}
                }
                return;
            }
            Mode::GotoLine(_) => {
                self.handle_goto_key(k);
                return;
            }
            Mode::SaveAs(_) => {
                self.handle_save_as_key(k);
                return;
            }
            Mode::Find => {
                self.handle_find_key(k, false);
                return;
            }
            Mode::Replace { focus_replace } => {
                let fr = *focus_replace;
                self.handle_find_key(k, fr);
                return;
            }
            Mode::Edit => {}
        }

        // ---- Main Edit Mode Keybindings (100% VS Code aligned) ----
        let page_step = rows.saturating_sub(4).max(5);

        // Ctrl+Shift combinations
        if k.ctrl && k.shift && !k.alt {
            match k.code {
                KeyCode::Char('z') => {
                    if self.doc_mut().redo() {
                        self.set_status("Refeito");
                    }
                    return;
                }
                KeyCode::Char('k') => {
                    self.doc_mut().delete_lines();
                    return;
                }
                KeyCode::Char('d') => {
                    self.doc_mut().duplicate(false);
                    return;
                }
                KeyCode::Char('l') => {
                    self.doc_mut().select_all_occurrences();
                    let n = self.doc().cursors.len();
                    if n > 1 {
                        self.set_status(format!("{} ocorrencias selecionadas", n));
                    }
                    return;
                }
                KeyCode::Char('p') => {
                    self.open_command_palette();
                    return;
                }
                KeyCode::Left => {
                    self.doc_mut().move_cursors(Motion::WordLeft, true);
                    return;
                }
                KeyCode::Right => {
                    self.doc_mut().move_cursors(Motion::WordRight, true);
                    return;
                }
                KeyCode::Home => {
                    self.doc_mut().move_cursors(Motion::DocStart, true);
                    return;
                }
                KeyCode::End => {
                    self.doc_mut().move_cursors(Motion::DocEnd, true);
                    return;
                }
                KeyCode::Backspace => {
                    self.doc_mut().delete_lines();
                    return;
                }
                _ => {}
            }
        }

        // Ctrl+Alt combinations (VS Code multi-cursor vertical!)
        if k.ctrl && k.alt && !k.shift {
            match k.code {
                KeyCode::Up => {
                    self.doc_mut().add_cursor_vertical(true);
                    return;
                }
                KeyCode::Down => {
                    self.doc_mut().add_cursor_vertical(false);
                    return;
                }
                _ => {}
            }
        }

        // Shift+Alt combinations (VS Code duplicate line up/down)
        if k.alt && k.shift && !k.ctrl {
            match k.code {
                KeyCode::Down => {
                    self.doc_mut().duplicate(false);
                    return;
                }
                KeyCode::Up => {
                    self.doc_mut().duplicate(true);
                    return;
                }
                _ => {}
            }
        }

        // Alt combinations
        if k.alt && !k.ctrl && !k.shift {
            match k.code {
                KeyCode::Up => {
                    self.doc_mut().move_lines(true);
                    return;
                }
                KeyCode::Down => {
                    self.doc_mut().move_lines(false);
                    return;
                }
                KeyCode::Left => {
                    if self.tabs.len() > 1 {
                        self.active_tab = (self.active_tab + self.tabs.len() - 1) % self.tabs.len();
                    } else {
                        self.doc_mut().move_cursors(Motion::WordLeft, false);
                    }
                    return;
                }
                KeyCode::Right => {
                    if self.tabs.len() > 1 {
                        self.active_tab = (self.active_tab + 1) % self.tabs.len();
                    } else {
                        self.doc_mut().move_cursors(Motion::WordRight, false);
                    }
                    return;
                }
                KeyCode::Char('z') => {
                    self.cfg.wrap = !self.cfg.wrap;
                    self.cfg.save();
                    self.set_status(format!("Word Wrap: {}", if self.cfg.wrap { "ON" } else { "OFF" }));
                    return;
                }
                KeyCode::Char('v') => {
                    self.cfg.toggle_vim_visual();
                    self.term_sync_needed = true;
                    let st = if self.cfg.vim_visual { "ON (relnum + scrollbar + sem mouse)" } else { "OFF (VS Code padrao)" };
                    self.set_status(format!("Estilo Vim Visual: {}", st));
                    return;
                }
                KeyCode::Char('m') => {
                    let head = self.doc().primary_cursor().head;
                    if let Some((_a, b)) = syntax::find_matching_bracket(&self.doc().rope, head) {
                        let doc = self.doc_mut();
                        doc.cursors = vec![document::Selection::caret(b)];
                        doc.primary = 0;
                    }
                    return;
                }
                KeyCode::Char('f') => {
                    self.enter_find(false);
                    return;
                }
                KeyCode::Char('r') | KeyCode::Char('h') => {
                    self.enter_find(true);
                    return;
                }
                KeyCode::Backspace => {
                    self.doc_mut().backspace(true);
                    return;
                }
                _ => {}
            }
        }

        // Ctrl combinations
        if k.ctrl && !k.alt && !k.shift {
            match k.code {
                KeyCode::Char('s') => {
                    self.try_save_active();
                    return;
                }
                KeyCode::Char('q') => {
                    if self.tabs.iter().any(|t| t.is_dirty()) {
                        self.mode = Mode::ConfirmQuit;
                    } else {
                        self.should_quit = true;
                    }
                    return;
                }
                KeyCode::Char('w') => {
                    if self.doc().is_dirty() {
                        self.mode = Mode::ConfirmCloseTab;
                    } else {
                        self.close_active_tab_force();
                    }
                    return;
                }
                KeyCode::Char('n') => {
                    self.tabs.push(Document::new_empty());
                    self.active_tab = self.tabs.len() - 1;
                    self.set_status("Nova aba criada");
                    return;
                }
                KeyCode::Char('p') => {
                    let base = self.base_dir();
                    self.mode = Mode::Picker(Picker::quick_open(&base));
                    return;
                }
                KeyCode::Char('f') => {
                    self.enter_find(false);
                    return;
                }
                KeyCode::Char('g') => {
                    self.mode = Mode::GotoLine(String::new());
                    return;
                }
                KeyCode::Char('a') => {
                    self.doc_mut().select_all();
                    return;
                }
                KeyCode::Char('l') => {
                    self.doc_mut().select_line();
                    return;
                }
                KeyCode::Char('d') => {
                    self.doc_mut().select_next_occurrence();
                    let n = self.doc().cursors.len();
                    if n > 1 {
                        self.set_status(format!("{} cursores ativos (Esc para sair)", n));
                    }
                    return;
                }
                KeyCode::Char('z') => {
                    if self.doc_mut().undo() {
                        self.set_status("Desfeito");
                    }
                    return;
                }
                KeyCode::Char('y') => {
                    if self.doc_mut().redo() {
                        self.set_status("Refeito");
                    }
                    return;
                }
                KeyCode::Char('c') => {
                    let txt = self.doc().copy_text();
                    if !txt.is_empty() {
                        self.clipboard = txt.clone();
                        copy_to_system_clipboard(&txt);
                        self.set_status(format!("Copiado ({} chars)", txt.chars().count()));
                    }
                    return;
                }
                KeyCode::Char('x') => {
                    let txt = self.doc_mut().cut_text();
                    if !txt.is_empty() {
                        self.clipboard = txt.clone();
                        copy_to_system_clipboard(&txt);
                        self.set_status(format!("Recortado ({} chars)", txt.chars().count()));
                    }
                    return;
                }
                KeyCode::Char('v') => {
                    let txt = paste_from_system_clipboard().unwrap_or_else(|| self.clipboard.clone());
                    if !txt.is_empty() {
                        self.doc_mut().paste(&txt);
                    }
                    return;
                }
                KeyCode::Char('/') => {
                    self.doc_mut().toggle_comment();
                    return;
                }
                KeyCode::Left => {
                    self.doc_mut().move_cursors(Motion::WordLeft, false);
                    return;
                }
                KeyCode::Right => {
                    self.doc_mut().move_cursors(Motion::WordRight, false);
                    return;
                }
                KeyCode::Up => {
                    self.doc_mut().move_cursors(Motion::PageUp(5), false);
                    return;
                }
                KeyCode::Down => {
                    self.doc_mut().move_cursors(Motion::PageDown(5), false);
                    return;
                }
                KeyCode::Home => {
                    self.doc_mut().move_cursors(Motion::DocStart, false);
                    return;
                }
                KeyCode::End => {
                    self.doc_mut().move_cursors(Motion::DocEnd, false);
                    return;
                }
                KeyCode::Backspace => {
                    self.doc_mut().backspace(true);
                    return;
                }
                KeyCode::Delete => {
                    self.doc_mut().delete(true);
                    return;
                }
                _ => {}
            }
        }

        // Plain & Shift keys
        match k.code {
            KeyCode::Esc => {
                if !self.doc_mut().collapse_cursors() && !self.doc().search.query.is_empty() {
                    self.doc_mut().search.query.clear();
                    self.doc_mut().search.matches.clear();
                    self.doc_mut().search.active_idx = None;
                }
            }
            KeyCode::F(1) => self.open_command_palette(),
            KeyCode::F(2) => {
                let next_idx = palette::PALETTES
                    .iter()
                    .position(|p| p.name == self.palette.name)
                    .map(|i| (i + 1) % palette::PALETTES.len())
                    .unwrap_or(0);
                self.palette = Palette::by_index(next_idx);
                self.set_status(format!("Paleta: {}", self.palette.name));
            }
            KeyCode::F(3) => {
                let m = if k.shift {
                    self.doc_mut().search.prev_match()
                } else {
                    self.doc_mut().search.next_match()
                };
                if let Some((s, e)) = m {
                    self.select_byte_range(s, e);
                }
            }
            KeyCode::F(5) => {
                if let Err(e) = self.doc_mut().reload_from_disk() {
                    self.set_status(format!("Erro ao recarregar: {}", e));
                } else {
                    self.set_status("Recarregado do disco");
                }
            }
            KeyCode::Left => self.doc_mut().move_cursors(Motion::Left, k.shift),
            KeyCode::Right => self.doc_mut().move_cursors(Motion::Right, k.shift),
            KeyCode::Up => self.doc_mut().move_cursors(Motion::Up, k.shift),
            KeyCode::Down => self.doc_mut().move_cursors(Motion::Down, k.shift),
            KeyCode::Home => self.doc_mut().move_cursors(Motion::Home, k.shift),
            KeyCode::End => self.doc_mut().move_cursors(Motion::End, k.shift),
            KeyCode::PageUp => self.doc_mut().move_cursors(Motion::PageUp(page_step), k.shift),
            KeyCode::PageDown => self.doc_mut().move_cursors(Motion::PageDown(page_step), k.shift),
            KeyCode::Enter => {
                if k.shift {
                    self.doc_mut().insert_line_below();
                } else {
                    self.doc_mut().insert_newline();
                }
            }
            KeyCode::Tab => self.doc_mut().indent(false),
            KeyCode::BackTab => self.doc_mut().indent(true),
            KeyCode::Backspace => self.doc_mut().backspace(false),
            KeyCode::Delete => self.doc_mut().delete(false),
            KeyCode::Char(ch) if !k.ctrl && !k.alt => self.doc_mut().insert_char(ch),
            _ => {}
        }
    }

    fn open_command_palette(&mut self) {
        let pk = Picker::command_palette(&self.cfg, self.doc().eol.label());
        self.mode = Mode::Picker(pk);
    }

    fn enter_find(&mut self, replace: bool) {
        // Pre-fill query with active single-line selection if any (like VS Code!)
        let p = self.doc().primary_cursor();
        if !p.is_empty() {
            let (s, e) = p.range();
            let sel = self.doc().rope.slice(s, e);
            if !sel.contains('\n') && sel.len() <= 120 {
                self.doc_mut().search.query = sel;
            }
        }
        self.recompute_search_and_jump();
        self.mode = if replace {
            Mode::Replace { focus_replace: true }
        } else {
            Mode::Find
        };
    }

    fn recompute_search_and_jump(&mut self) {
        let cur = self.doc().primary_cursor().head;
        let doc = self.doc_mut();
        doc.search.recompute(&doc.rope, cur);
        if let Some((s, e)) = doc.search.active_match() {
            self.select_byte_range(s, e);
        }
    }

    fn select_byte_range(&mut self, s: usize, e: usize) {
        let doc = self.doc_mut();
        doc.cursors = vec![document::Selection {
            anchor: s,
            head: e,
            desired_col: 0,
        }];
        doc.primary = 0;
    }

    fn handle_find_key(&mut self, k: KeyEvent, focus_replace: bool) {
        if k.alt && !k.ctrl {
            match k.code {
                KeyCode::Char('c') => {
                    self.doc_mut().search.case_sensitive = !self.doc().search.case_sensitive;
                    self.recompute_search_and_jump();
                    return;
                }
                KeyCode::Char('w') => {
                    self.doc_mut().search.whole_word = !self.doc().search.whole_word;
                    self.recompute_search_and_jump();
                    return;
                }
                KeyCode::Char('r') => {
                    self.doc_mut().search.use_regex = !self.doc().search.use_regex;
                    self.recompute_search_and_jump();
                    return;
                }
                KeyCode::Char('a') => {
                    let n = self.doc_mut().replace_all_matches();
                    self.set_status(format!("{} ocorrencia(s) substituida(s)", n));
                    return;
                }
                _ => {}
            }
        }

        match k.code {
            KeyCode::Esc => {
                self.mode = Mode::Edit;
            }
            KeyCode::Tab => {
                self.mode = match self.mode {
                    Mode::Find => Mode::Replace { focus_replace: true },
                    Mode::Replace { focus_replace: fr } => Mode::Replace { focus_replace: !fr },
                    _ => Mode::Find,
                };
            }
            KeyCode::Enter => {
                if focus_replace {
                    if self.doc_mut().replace_active_match() {
                        if let Some((s, e)) = self.doc().search.active_match() {
                            self.select_byte_range(s, e);
                        }
                    }
                } else {
                    let m = if k.shift {
                        self.doc_mut().search.prev_match()
                    } else {
                        self.doc_mut().search.next_match()
                    };
                    if let Some((s, e)) = m {
                        self.select_byte_range(s, e);
                    }
                }
            }
            KeyCode::F(3) => {
                let m = if k.shift {
                    self.doc_mut().search.prev_match()
                } else {
                    self.doc_mut().search.next_match()
                };
                if let Some((s, e)) = m {
                    self.select_byte_range(s, e);
                }
            }
            KeyCode::Backspace => {
                if focus_replace {
                    self.doc_mut().search.replace.pop();
                } else {
                    self.doc_mut().search.query.pop();
                    self.recompute_search_and_jump();
                }
            }
            KeyCode::Char(c) if !k.ctrl && !k.alt => {
                if focus_replace {
                    self.doc_mut().search.replace.push(c);
                } else {
                    self.doc_mut().search.query.push(c);
                    self.recompute_search_and_jump();
                }
            }
            _ => {}
        }
    }

    fn handle_goto_key(&mut self, k: KeyEvent) {
        let Mode::GotoLine(ref mut buf) = self.mode else { return };
        match k.code {
            KeyCode::Esc => self.mode = Mode::Edit,
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Char(c) if c.is_ascii_digit() => buf.push(c),
            KeyCode::Enter => {
                let target = buf.parse::<usize>().ok();
                self.mode = Mode::Edit;
                if let Some(ln) = target {
                    self.doc_mut().goto_line(ln);
                    self.set_status(format!("Linha {}", ln));
                }
            }
            _ => {}
        }
    }

    fn handle_save_as_key(&mut self, k: KeyEvent) {
        let Mode::SaveAs(ref mut buf) = self.mode else { return };
        match k.code {
            KeyCode::Esc => self.mode = Mode::Edit,
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Char(c) if !k.ctrl && !k.alt => buf.push(c),
            KeyCode::Enter => {
                let p = buf.trim().to_string();
                self.mode = Mode::Edit;
                if !p.is_empty() {
                    let path = PathBuf::from(&p);
                    match self.doc_mut().save_as(&path) {
                        Ok(()) => self.set_status(format!("✓ Salvo em {}", p)),
                        Err(e) => self.set_status(format!("Erro ao salvar: {}", e)),
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_picker_key(&mut self, k: KeyEvent) {
        let action = if let Mode::Picker(ref mut pk) = self.mode {
            match k.code {
                KeyCode::Esc => {
                    self.mode = Mode::Edit;
                    return;
                }
                KeyCode::Up => {
                    pk.move_sel(-1);
                    return;
                }
                KeyCode::Down => {
                    pk.move_sel(1);
                    return;
                }
                KeyCode::Char('p') if k.ctrl => {
                    pk.move_sel(-1);
                    return;
                }
                KeyCode::Char('n') if k.ctrl => {
                    pk.move_sel(1);
                    return;
                }
                KeyCode::Backspace => {
                    pk.query.pop();
                    pk.refilter();
                    return;
                }
                KeyCode::Char(c) if !k.ctrl && !k.alt => {
                    pk.query.push(c);
                    pk.refilter();
                    return;
                }
                KeyCode::Enter => pk.chosen(),
                _ => return,
            }
        } else {
            return;
        };

        self.mode = Mode::Edit;
        if let Some(act) = action {
            match act {
                PickerAction::OpenFile(path) => self.open_or_focus_file(&path, None),
                PickerAction::ToggleVimVisual => {
                    self.cfg.toggle_vim_visual();
                    self.term_sync_needed = true;
                    let st = if self.cfg.vim_visual { "ON (relnum + bloco + sem mouse)" } else { "OFF (VS Code padrao)" };
                    self.set_status(format!("Estilo Vim Visual: {}", st));
                }
                PickerAction::ToggleRelativeNumbers => {
                    self.cfg.relative_numbers = !self.cfg.relative_numbers;
                    self.cfg.save();
                    let st = if self.cfg.relative_numbers { "ON" } else { "OFF" };
                    self.set_status(format!("Numeros Relativos: {}", st));
                }
                PickerAction::ToggleBlockCursor => {
                    self.cfg.block_cursor = !self.cfg.block_cursor;
                    self.cfg.save();
                    self.term_sync_needed = true;
                    let st = if self.cfg.block_cursor { "Bloco (█)" } else { "Barra (|)" };
                    self.set_status(format!("Cursor: {}", st));
                }
                PickerAction::ToggleMouse => {
                    self.cfg.mouse = !self.cfg.mouse;
                    self.cfg.save();
                    self.term_sync_needed = true;
                    let st = if self.cfg.mouse { "ON" } else { "OFF (teclado puro)" };
                    self.set_status(format!("Mouse: {}", st));
                }
                PickerAction::ToggleScrollbar => {
                    self.cfg.scrollbar = !self.cfg.scrollbar;
                    self.cfg.save();
                    let st = if self.cfg.scrollbar { "ON" } else { "OFF" };
                    self.set_status(format!("Scrollbar lateral: {}", st));
                }
                PickerAction::ToggleCenterScroll => {
                    self.cfg.center_scroll = !self.cfg.center_scroll;
                    self.cfg.save();
                    let st = if self.cfg.center_scroll { "ON (typewriter)" } else { "OFF" };
                    self.set_status(format!("Scroll centralizado: {}", st));
                }
                PickerAction::ToggleWrap => {
                    self.cfg.wrap = !self.cfg.wrap;
                    self.cfg.save();
                    self.set_status(format!("Word Wrap: {}", if self.cfg.wrap { "ON" } else { "OFF" }));
                }
                PickerAction::ToggleAutoClose => {
                    self.cfg.auto_close = !self.cfg.auto_close;
                    self.cfg.save();
                    for t in &mut self.tabs {
                        t.auto_close = self.cfg.auto_close;
                    }
                    let st = if self.cfg.auto_close { "ON" } else { "OFF" };
                    self.set_status(format!("Auto-Close Pares: {}", st));
                }
                PickerAction::ToggleLineEnding => {
                    let next = match self.doc().eol {
                        LineEnding::Lf => LineEnding::Crlf,
                        LineEnding::Crlf => LineEnding::Lf,
                    };
                    self.doc_mut().eol = next;
                    self.set_status(format!("Final de linha alterado para {}", next.label()));
                }
                PickerAction::TrimTrailingSpaces => {
                    let n = self.doc_mut().trim_trailing_whitespace();
                    self.set_status(format!("Removidos {} espaco(s) no fim das linhas", n));
                }
                PickerAction::ReloadFromDisk => {
                    match self.doc_mut().reload_from_disk() {
                        Ok(()) => self.set_status("Recarregado do disco"),
                        Err(e) => self.set_status(format!("Erro: {}", e)),
                    }
                }
                PickerAction::GotoLine => self.mode = Mode::GotoLine(String::new()),
                PickerAction::SaveAs => {
                    let cur = self
                        .doc()
                        .path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.mode = Mode::SaveAs(cur);
                }
                PickerAction::NewBuffer => {
                    self.tabs.push(Document::new_empty());
                    self.active_tab = self.tabs.len() - 1;
                }
                PickerAction::SetPalette(idx) => {
                    self.palette = Palette::by_index(idx);
                    self.set_status(format!("Paleta: {}", self.palette.name));
                }
            }
        }
    }

    fn try_save_active(&mut self) -> bool {
        if self.doc().path.is_none() {
            self.mode = Mode::SaveAs(String::new());
            return false;
        }
        match self.doc_mut().save() {
            Ok(()) => {
                let name = self.doc().display_name();
                self.set_status(format!("✓ Salvo: {}", name));
                true
            }
            Err(e) => {
                self.set_status(format!("Erro ao salvar: {}", e));
                false
            }
        }
    }

    fn save_all_dirty(&mut self) -> bool {
        for i in 0..self.tabs.len() {
            if self.tabs[i].is_dirty() {
                if self.tabs[i].path.is_none() {
                    self.active_tab = i;
                    self.mode = Mode::SaveAs(String::new());
                    return false;
                }
                if let Err(e) = self.tabs[i].save() {
                    self.active_tab = i;
                    self.set_status(format!("Erro ao salvar: {}", e));
                    return false;
                }
            }
        }
        true
    }

    fn close_active_tab_force(&mut self) {
        if self.tabs.len() <= 1 {
            self.should_quit = true;
        } else {
            self.tabs.remove(self.active_tab);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
        }
    }

    fn render(&mut self, cols: usize, rows: usize, synced: bool) -> String {
        let has_prompt = !matches!(self.mode, Mode::Edit | Mode::Picker(_));
        let cfg = self.cfg;
        ensure_cursor_visible(self.doc_mut(), cols, rows, &cfg, has_prompt);

        let status = self.active_status().map(str::to_string);
        let active_tab = self.active_tab;
        let pal = self.palette;

        // Temporarily extract active doc so we can pass `&tabs` and `&mut doc` cleanly
        let mut doc = std::mem::replace(&mut self.tabs[active_tab], Document::new_empty());
        let search_snap = doc.search.clone();
        let prompt = match &self.mode {
            Mode::Find => PromptBar::Find {
                query: &search_snap.query,
                count: search_snap.matches.len(),
                active: search_snap.active_idx,
                case: search_snap.case_sensitive,
                word: search_snap.whole_word,
                regex: search_snap.use_regex,
                error: search_snap.error.as_deref(),
            },
            Mode::Replace { focus_replace } => PromptBar::Replace {
                query: &search_snap.query,
                replace: &search_snap.replace,
                focus_replace: *focus_replace,
                count: search_snap.matches.len(),
                active: search_snap.active_idx,
                case: search_snap.case_sensitive,
                word: search_snap.whole_word,
                regex: search_snap.use_regex,
                error: search_snap.error.as_deref(),
            },
            Mode::GotoLine(v) => PromptBar::Input {
                label: "Ir para linha",
                value: v,
                hint: "Enter confirma · Esc cancela",
            },
            Mode::SaveAs(v) => PromptBar::Input {
                label: "Salvar como",
                value: v,
                hint: "caminho do arquivo · Enter confirma",
            },
            Mode::ConfirmQuit => PromptBar::Confirm {
                message: "Ha alteracoes nao salvas! Salvar antes de sair? [S]im / [N]ao / [Esc] Cancelar",
            },
            Mode::ConfirmCloseTab => PromptBar::Confirm {
                message: "Aba modificada! Salvar antes de fechar? [S]im / [N]ao / [Esc] Cancelar",
            },
            Mode::Edit | Mode::Picker(_) => PromptBar::None,
        };
        let picker_ref = match &self.mode {
            Mode::Picker(pk) => Some(pk),
            _ => None,
        };

        let (frame, layout) = render_editor(
            &self.tabs,
            active_tab,
            &mut doc,
            pal,
            cols,
            rows,
            &cfg,
            prompt,
            picker_ref,
            status.as_deref(),
            synced,
        );
        self.tabs[active_tab] = doc;
        self.layout = layout;
        frame
    }
}

fn parse_file_arg(arg: &str) -> (PathBuf, Option<usize>) {
    let p = Path::new(arg);
    if p.exists() {
        return (p.to_path_buf(), None);
    }
    if let Some((left, right)) = arg.rsplit_once(':') {
        if let Ok(ln) = right.parse::<usize>() {
            return (PathBuf::from(left), Some(ln));
        }
    }
    (p.to_path_buf(), None)
}

fn print_help() {
    println!(
        "{}\n\nUso:\n  nave-edit [opcoes] [arquivo[:linha] ...]\n  nave-edit +<linha> <arquivo>\n\nOpcoes:\n  -h, --help          Mostra esta ajuda\n  -v, --version       Mostra versao\n  --render-once <f>   Renderiza 1 frame estatico e sai\n\nAtalhos principais (VS Code):\n  Ctrl+S salvar · Ctrl+Q sair · Ctrl+W fechar aba · Ctrl+P abrir arquivo · F1 comandos\n  Ctrl+D multi-cursor · Ctrl+Shift+L todas ocorrencias · Ctrl+Alt+↑/↓ cursor acima/abaixo\n  Ctrl+F buscar (Tab substitui) · Ctrl+G ir p/ linha · Alt+V estilo Vim Visual · Alt+Z wrap · Alt+1..5 paletas",
        VERSION
    );
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut render_once = false;
    let mut pending_line: Option<usize> = None;
    let mut docs: Vec<Document> = Vec::new();

    for arg in &args {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return;
            }
            "-v" | "--version" => {
                println!("{}", VERSION);
                return;
            }
            "--render-once" => {
                render_once = true;
            }
            s if s.starts_with('+') && s[1..].chars().all(|c| c.is_ascii_digit()) && s.len() > 1 => {
                pending_line = s[1..].parse().ok();
            }
            other => {
                let (path, inline_ln) = parse_file_arg(other);
                let ln = pending_line.take().or(inline_ln);
                match Document::open(&path, ln) {
                    Ok(d) => docs.push(d),
                    Err(e) => {
                        eprintln!("nave-edit: erro ao abrir '{}': {}", other, e);
                        std::process::exit(1);
                    }
                }
            }
        }
    }

    let pal = Palette::load_active();
    let cfg = EditorConfig::load();
    let mut app = App::new(docs, pal, cfg);

    if render_once {
        let (cols, rows) = Terminal::size();
        let frame = app.render(cols, rows, false);
        print!("{}", frame);
        return;
    }

    let term = match Terminal::enter(app.cfg.mouse, app.cfg.block_cursor) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nave-edit: requer um terminal interativo (TTY): {}", e);
            std::process::exit(1);
        }
    };

    let mut decoder = InputDecoder::new();
    let mut read_buf = [0u8; 4096];
    let mut needs_redraw = true;
    let mut last_activity = Instant::now();
    let mut standby_blink = false;

    while !app.should_quit {
        let (cols, rows) = Terminal::size();

        if app.term_sync_needed {
            term.set_mouse(app.cfg.mouse);
            term.set_cursor_style(app.cfg.block_cursor, standby_blink);
            app.term_sync_needed = false;
            needs_redraw = true;
        }

        // Standby blink: solid cursor while actively typing/scrolling, gentle blink after 750ms idle
        if !standby_blink && last_activity.elapsed() >= Duration::from_millis(750) {
            standby_blink = true;
            term.set_cursor_style(app.cfg.block_cursor, true);
        }

        if Terminal::take_resized() {
            needs_redraw = true;
        }
        if app.doc().git.take_dirty() {
            needs_redraw = true;
        }
        if app.last_mtime_check.elapsed() >= Duration::from_secs(2) {
            let prev = app.doc().external_changed;
            app.doc_mut().check_external_change();
            app.last_mtime_check = Instant::now();
            if app.doc().external_changed != prev {
                needs_redraw = true;
            }
        }

        if needs_redraw {
            let frame = app.render(cols, rows, true);
            if term.write_frame(&frame).is_err() {
                break;
            }
            needs_redraw = false;
        }

        let poll_ms = if decoder.has_pending_esc() { 25 } else { 60 };
        if term.poll_stdin(poll_ms) {
            let n = term.read_bytes(&mut read_buf);
            if n > 0 {
                last_activity = Instant::now();
                if standby_blink {
                    standby_blink = false;
                    term.set_cursor_style(app.cfg.block_cursor, false);
                }
                decoder.push(&read_buf[..n]);
                while let Some(ev) = decoder.next_event() {
                    app.handle_event(ev, cols, rows);
                    needs_redraw = true;
                    if app.should_quit {
                        break;
                    }
                }
            }
        } else if decoder.has_pending_esc() {
            if let Some(ev) = decoder.flush_bare_esc() {
                app.handle_event(ev, cols, rows);
                needs_redraw = true;
            }
        }
    }
}
