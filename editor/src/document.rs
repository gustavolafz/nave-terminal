use crate::git::GitGutter;
use crate::rope::{LineEnding, Rope};
use crate::search::SearchState;
use crate::syntax::{Language, SyntaxCache};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,      // byte offset
    pub head: usize,        // byte offset (active cursor end)
    pub desired_col: usize, // char col remembered during vertical moves
}

impl Selection {
    pub fn caret(pos: usize) -> Self {
        Self {
            anchor: pos,
            head: pos,
            desired_col: 0,
        }
    }

    pub fn is_empty(self) -> bool {
        self.anchor == self.head
    }

    pub fn range(self) -> (usize, usize) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    Home,
    End,
    DocStart,
    DocEnd,
    PageUp(usize),
    PageDown(usize),
}

#[derive(Clone, Debug)]
struct UndoEntry {
    rope_before: Rope,
    cursors_before: Vec<Selection>,
    rope_after: Rope,
    cursors_after: Vec<Selection>,
    coalesce_key: Option<u8>,
    timestamp: Instant,
}

pub struct Document {
    pub path: Option<PathBuf>,
    pub rope: Rope,
    pub eol: LineEnding,
    pub lang: Language,
    pub syntax: SyntaxCache,
    pub git: GitGutter,
    pub cursors: Vec<Selection>,
    pub primary: usize,
    pub scroll_row: usize,
    pub scroll_col: usize,
    pub search: SearchState,
    pub bookmarks: BTreeSet<usize>,
    pub auto_close: bool,
    pub tab_width: usize,
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    rev: usize,
    saved_rev: usize,
    disk_mtime: Option<SystemTime>,
    pub external_changed: bool,
}

impl Document {
    pub fn new_empty() -> Self {
        Self {
            path: None,
            rope: Rope::new(),
            eol: LineEnding::Lf,
            lang: Language::Plain,
            syntax: SyntaxCache::new(Language::Plain),
            git: GitGutter::new(),
            cursors: vec![Selection::caret(0)],
            primary: 0,
            scroll_row: 0,
            scroll_col: 0,
            search: SearchState::default(),
            bookmarks: BTreeSet::new(),
            auto_close: true,
            tab_width: 2,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            rev: 0,
            saved_rev: 0,
            disk_mtime: None,
            external_changed: false,
        }
    }

    pub fn open(path: &Path, jump_line: Option<usize>) -> io::Result<Self> {
        let mut doc = Self::new_empty();
        doc.path = Some(path.to_path_buf());
        if path.exists() {
            let raw_bytes = fs::read(path)?;
            let raw = String::from_utf8_lossy(&raw_bytes);
            let (rope, eol) = Rope::from_str_detect_eol(&raw);
            doc.rope = rope;
            doc.eol = eol;
            doc.disk_mtime = fs::metadata(path).ok().and_then(|m| m.modified().ok());
        }
        let first_line = doc.rope.line(0);
        doc.lang = Language::detect(Some(path), &first_line);
        doc.tab_width = doc.lang.indent_width();
        doc.syntax.set_language(doc.lang);
        if path.exists() {
            doc.git.refresh_async(path);
        }
        if let Some(ln) = jump_line {
            doc.goto_line(ln);
        }
        Ok(doc)
    }

    pub fn display_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("[Sem nome]")
            .to_string()
    }

    pub fn is_dirty(&self) -> bool {
        self.rev != self.saved_rev
    }

    pub fn primary_cursor(&self) -> Selection {
        self.cursors[self.primary.min(self.cursors.len() - 1)]
    }

    pub fn primary_line_col(&self) -> (usize, usize) {
        self.rope.byte_to_line_col(self.primary_cursor().head)
    }

    pub fn clamp_byte_pos(&self, byte_pos: usize) -> usize {
        let clamped = byte_pos.min(self.rope.len_bytes());
        let (r, c) = self.rope.byte_to_line_col(clamped);
        self.rope.line_col_to_byte(r, c)
    }

    pub fn clamp_view_state(
        &self,
        cursors: &mut Vec<Selection>,
        primary: &mut usize,
        scroll_row: &mut usize,
    ) {
        if cursors.is_empty() {
            cursors.push(Selection::caret(0));
        }
        for sel in cursors.iter_mut() {
            sel.anchor = self.clamp_byte_pos(sel.anchor);
            sel.head = self.clamp_byte_pos(sel.head);
        }
        if *primary >= cursors.len() {
            *primary = cursors.len() - 1;
        }
        let max_r = self.rope.line_count().saturating_sub(1);
        if *scroll_row > max_r {
            *scroll_row = max_r;
        }
    }

    pub fn reload_from_disk(&mut self) -> io::Result<()> {
        let Some(ref path) = self.path.clone() else {
            return Ok(());
        };
        let raw_bytes = fs::read(path)?;
        let raw = String::from_utf8_lossy(&raw_bytes);
        let (rope, eol) = Rope::from_str_detect_eol(&raw);
        let (old_line, old_col) = self.primary_line_col();
        self.rope = rope;
        self.eol = eol;
        self.disk_mtime = fs::metadata(path).ok().and_then(|m| m.modified().ok());
        self.external_changed = false;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.rev = 0;
        self.saved_rev = 0;
        let clamped_byte = self.rope.line_col_to_byte(old_line, old_col);
        self.cursors = vec![Selection::caret(clamped_byte)];
        self.primary = 0;
        self.syntax.invalidate_from(0);
        self.git.refresh_async(path);
        Ok(())
    }

    pub fn check_external_change(&mut self) {
        if self.external_changed {
            return;
        }
        if let (Some(ref p), Some(prev)) = (&self.path, self.disk_mtime) {
            if let Ok(meta) = fs::metadata(p) {
                if let Ok(cur) = meta.modified() {
                    if cur != prev {
                        self.external_changed = true;
                    }
                }
            }
        }
    }

    pub fn save(&mut self) -> io::Result<()> {
        let path = self.path.clone().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Nenhum caminho definido")
        })?;
        self.save_as(&path)
    }

    pub fn save_as(&mut self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let existing_mode = fs::metadata(path).ok().map(|m| m.permissions().mode());
        let content = self.rope.to_string_with_eol(self.eol);

        // Atomic write via sibling temp file + rename
        let tmp = path.with_extension(format!(
            "{}.nave-tmp-{}",
            path.extension().and_then(|e| e.to_str()).unwrap_or("tmp"),
            std::process::id()
        ));
        fs::write(&tmp, content.as_bytes())?;
        if let Some(mode) = existing_mode {
            let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(mode));
        }
        fs::rename(&tmp, path)?;

        self.path = Some(path.to_path_buf());
        let first_line = self.rope.line(0);
        let new_lang = Language::detect(Some(path), &first_line);
        if new_lang != self.lang {
            self.lang = new_lang;
            self.tab_width = new_lang.indent_width();
            self.syntax.set_language(new_lang);
        }
        self.saved_rev = self.rev;
        self.disk_mtime = fs::metadata(path).ok().and_then(|m| m.modified().ok());
        self.external_changed = false;
        self.git.refresh_async(path);
        Ok(())
    }

    fn record_tx<F>(&mut self, coalesce_key: Option<u8>, f: F)
    where
        F: FnOnce(&mut Self),
    {
        let rope_before = self.rope.clone();
        let cursors_before = self.cursors.clone();
        let dirty_line_hint = self.cursors.iter().map(|c| self.rope.byte_to_line_col(c.range().0).0).min().unwrap_or(0);

        f(self);
        self.normalize_cursors();
        self.syntax.invalidate_from(dirty_line_hint.saturating_sub(1));
        self.redo_stack.clear();

        let now = Instant::now();
        let can_coalesce = coalesce_key.is_some()
            && self.undo_stack.last().is_some_and(|last| {
                last.coalesce_key == coalesce_key
                    && now.duration_since(last.timestamp).as_millis() < 650
                    && self.rev != self.saved_rev
            });

        if can_coalesce {
            if let Some(last) = self.undo_stack.last_mut() {
                last.rope_after = self.rope.clone();
                last.cursors_after = self.cursors.clone();
                last.timestamp = now;
                return;
            }
        }

        self.rev += 1;
        self.undo_stack.push(UndoEntry {
            rope_before,
            cursors_before,
            rope_after: self.rope.clone(),
            cursors_after: self.cursors.clone(),
            coalesce_key,
            timestamp: now,
        });
        if self.undo_stack.len() > 250 {
            self.undo_stack.remove(0);
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo_stack.pop() else {
            return false;
        };
        self.rope = entry.rope_before.clone();
        self.cursors = entry.cursors_before.clone();
        self.primary = 0;
        self.rev = self.rev.saturating_sub(1);
        self.redo_stack.push(entry);
        self.syntax.invalidate_from(0);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(entry) = self.redo_stack.pop() else {
            return false;
        };
        self.rope = entry.rope_after.clone();
        self.cursors = entry.cursors_after.clone();
        self.primary = 0;
        self.rev += 1;
        self.undo_stack.push(entry);
        self.syntax.invalidate_from(0);
        true
    }

    pub fn normalize_cursors(&mut self) {
        let max_b = self.rope.len_bytes();
        for c in &mut self.cursors {
            c.anchor = c.anchor.min(max_b);
            c.head = c.head.min(max_b);
        }
        if self.cursors.is_empty() {
            self.cursors.push(Selection::caret(0));
            self.primary = 0;
            return;
        }
        let primary_head = self.cursors[self.primary.min(self.cursors.len() - 1)].head;
        self.cursors.sort_by_key(|c| (c.range().0, c.range().1));

        let mut merged: Vec<Selection> = Vec::with_capacity(self.cursors.len());
        for c in self.cursors.drain(..) {
            if let Some(last) = merged.last_mut() {
                let (ls, le) = last.range();
                let (cs, ce) = c.range();
                let overlaps = if last.is_empty() && c.is_empty() {
                    ls == cs
                } else {
                    cs < le || (cs == le && (last.is_empty() || c.is_empty()))
                };
                if overlaps {
                    let ns = ls.min(cs);
                    let ne = le.max(ce);
                    if last.head >= last.anchor {
                        last.anchor = ns;
                        last.head = ne;
                    } else {
                        last.anchor = ne;
                        last.head = ns;
                    }
                    continue;
                }
            }
            merged.push(c);
        }
        self.cursors = merged;
        self.primary = self
            .cursors
            .iter()
            .position(|c| c.head == primary_head)
            .unwrap_or(self.cursors.len().saturating_sub(1));
    }

    pub fn collapse_cursors(&mut self) -> bool {
        let mut changed = false;
        if self.cursors.len() > 1 {
            let p = self.primary_cursor();
            self.cursors = vec![p];
            self.primary = 0;
            changed = true;
        }
        if !self.cursors[0].is_empty() {
            self.cursors[0].anchor = self.cursors[0].head;
            changed = true;
        }
        changed
    }

    pub fn goto_line(&mut self, line_1based: usize) {
        let row = line_1based.saturating_sub(1).min(self.rope.line_count().saturating_sub(1));
        let b = self.rope.line_start_byte(row);
        self.cursors = vec![Selection::caret(b)];
        self.primary = 0;
    }

    pub fn select_all(&mut self) {
        let end = self.rope.len_bytes();
        self.cursors = vec![Selection {
            anchor: 0,
            head: end,
            desired_col: 0,
        }];
        self.primary = 0;
    }

    pub fn select_line(&mut self) {
        let c = self.primary_cursor();
        let (s_row, _) = self.rope.byte_to_line_col(c.range().0);
        let (e_row, e_col) = self.rope.byte_to_line_col(c.range().1);
        let next_row = if !c.is_empty() && e_col == 0 && e_row > s_row {
            (e_row + 1).min(self.rope.line_count())
        } else {
            (e_row + 1).min(self.rope.line_count())
        };
        let start_b = self.rope.line_start_byte(s_row);
        let end_b = if next_row < self.rope.line_count() {
            self.rope.line_start_byte(next_row)
        } else {
            self.rope.len_bytes()
        };
        self.cursors = vec![Selection {
            anchor: start_b,
            head: end_b,
            desired_col: 0,
        }];
        self.primary = 0;
    }

    /// VS Code Ctrl+D:
    /// - If primary cursor has no selection, select the word under/before cursor.
    /// - If already selected, find the next identical occurrence (wrapping around) and add a new cursor!
    pub fn select_next_occurrence(&mut self) {
        let p = self.primary_cursor();
        if p.is_empty() {
            if let Some((s, e)) = self.word_bounds_at(p.head) {
                let idx = self.primary;
                self.cursors[idx] = Selection {
                    anchor: s,
                    head: e,
                    desired_col: 0,
                };
            }
            return;
        }
        let (ps, pe) = p.range();
        let needle = self.rope.slice(ps, pe);
        if needle.is_empty() {
            return;
        }
        let full = self.rope.slice(0, self.rope.len_bytes());
        let search_from = self.cursors.last().map(|c| c.range().1).unwrap_or(pe);

        let found = full[search_from..]
            .find(&needle)
            .map(|off| search_from + off)
            .or_else(|| full[..ps].find(&needle));

        if let Some(start) = found {
            let end = start + needle.len();
            if !self.cursors.iter().any(|c| c.range() == (start, end)) {
                self.cursors.push(Selection {
                    anchor: start,
                    head: end,
                    desired_col: 0,
                });
                self.primary = self.cursors.len() - 1;
                self.normalize_cursors();
            }
        }
    }

    /// VS Code Ctrl+Shift+L: select ALL occurrences of current selection (or word under cursor).
    pub fn select_all_occurrences(&mut self) {
        if self.primary_cursor().is_empty() {
            self.select_next_occurrence();
        }
        let (ps, pe) = self.primary_cursor().range();
        if ps == pe {
            return;
        }
        let needle = self.rope.slice(ps, pe);
        let full = self.rope.slice(0, self.rope.len_bytes());
        let mut news = Vec::new();
        let mut off = 0;
        while let Some(rel) = full[off..].find(&needle) {
            let s = off + rel;
            let e = s + needle.len();
            news.push(Selection {
                anchor: s,
                head: e,
                desired_col: 0,
            });
            off = e;
            if news.len() >= 1000 {
                break;
            }
        }
        if !news.is_empty() {
            self.cursors = news;
            self.primary = 0;
            self.normalize_cursors();
        }
    }

    /// VS Code Ctrl+Alt+Up / Ctrl+Alt+Down: add a cursor on the line above/below.
    pub fn add_cursor_vertical(&mut self, up: bool) {
        let base = if up {
            self.cursors.first().copied()
        } else {
            self.cursors.last().copied()
        };
        let Some(c) = base else { return };
        let (row, col) = self.rope.byte_to_line_col(c.head);
        let target_col = if c.desired_col > 0 { c.desired_col } else { col };
        let next_row = if up {
            if row == 0 { return; }
            row - 1
        } else {
            if row + 1 >= self.rope.line_count() { return; }
            row + 1
        };
        let nb = self.rope.line_col_to_byte(next_row, target_col);
        self.cursors.push(Selection {
            anchor: nb,
            head: nb,
            desired_col: target_col,
        });
        self.primary = self.cursors.len() - 1;
        self.normalize_cursors();
    }

    pub fn set_cursor_at_line_col(&mut self, row: usize, col: usize, extend: bool) {
        let nb = self.rope.line_col_to_byte(row, col);
        if extend {
            let p = self.primary_cursor();
            self.cursors = vec![Selection {
                anchor: p.anchor,
                head: nb,
                desired_col: col,
            }];
        } else {
            self.cursors = vec![Selection {
                anchor: nb,
                head: nb,
                desired_col: col,
            }];
        }
        self.primary = 0;
    }

    pub fn select_word_at_line_col(&mut self, row: usize, col: usize) {
        let b = self.rope.line_col_to_byte(row, col);
        if let Some((s, e)) = self.word_bounds_at(b) {
            self.cursors = vec![Selection {
                anchor: s,
                head: e,
                desired_col: 0,
            }];
            self.primary = 0;
        }
    }

    pub fn select_full_line_at(&mut self, row: usize) {
        let s = self.rope.line_start_byte(row);
        let e = if row + 1 < self.rope.line_count() {
            self.rope.line_start_byte(row + 1)
        } else {
            self.rope.len_bytes()
        };
        self.cursors = vec![Selection {
            anchor: s,
            head: e,
            desired_col: 0,
        }];
        self.primary = 0;
    }

    pub fn move_cursors(&mut self, motion: Motion, extend: bool) {
        let rope = &self.rope;
        let line_count = rope.line_count();
        for c in &mut self.cursors {
            // Collapse selection cleanly on plain Left/Right (like VS Code)
            if !extend && !c.is_empty() {
                match motion {
                    Motion::Left => {
                        let s = c.range().0;
                        c.anchor = s;
                        c.head = s;
                        c.desired_col = rope.byte_to_line_col(s).1;
                        continue;
                    }
                    Motion::Right => {
                        let e = c.range().1;
                        c.anchor = e;
                        c.head = e;
                        c.desired_col = rope.byte_to_line_col(e).1;
                        continue;
                    }
                    _ => {}
                }
            }

            let (row, col) = rope.byte_to_line_col(c.head);
            let mut new_desired = col;
            let new_head = match motion {
                Motion::Left => prev_char_byte(rope, c.head),
                Motion::Right => next_char_byte(rope, c.head),
                Motion::WordLeft => prev_subword_byte(rope, c.head),
                Motion::WordRight => next_subword_byte(rope, c.head),
                Motion::Up => {
                    let target_col = c.desired_col.max(col);
                    new_desired = target_col;
                    if row == 0 {
                        0
                    } else {
                        rope.line_col_to_byte(row - 1, target_col)
                    }
                }
                Motion::Down => {
                    let target_col = c.desired_col.max(col);
                    new_desired = target_col;
                    if row + 1 >= line_count {
                        rope.len_bytes()
                    } else {
                        rope.line_col_to_byte(row + 1, target_col)
                    }
                }
                Motion::PageUp(step) => {
                    let target_col = c.desired_col.max(col);
                    new_desired = target_col;
                    rope.line_col_to_byte(row.saturating_sub(step), target_col)
                }
                Motion::PageDown(step) => {
                    let target_col = c.desired_col.max(col);
                    new_desired = target_col;
                    rope.line_col_to_byte((row + step).min(line_count - 1), target_col)
                }
                Motion::Home => {
                    // Smart VS Code Home: toggle between first non-whitespace char and col 0
                    let line_txt = rope.line(row);
                    let first_non_ws = line_txt.chars().take_while(|ch| ch.is_whitespace()).count();
                    let target = if col != first_non_ws && first_non_ws < line_txt.chars().count() {
                        first_non_ws
                    } else {
                        0
                    };
                    new_desired = target;
                    rope.line_col_to_byte(row, target)
                }
                Motion::End => {
                    let e = rope.line_end_byte(row);
                    new_desired = rope.byte_to_line_col(e).1;
                    e
                }
                Motion::DocStart => {
                    new_desired = 0;
                    0
                }
                Motion::DocEnd => {
                    let e = rope.len_bytes();
                    new_desired = rope.byte_to_line_col(e).1;
                    e
                }
            };

            c.head = new_head;
            if !extend {
                c.anchor = new_head;
            }
            if matches!(motion, Motion::Up | Motion::Down | Motion::PageUp(_) | Motion::PageDown(_)) {
                c.desired_col = new_desired;
            } else {
                c.desired_col = rope.byte_to_line_col(new_head).1;
            }
        }
        self.normalize_cursors();
    }

    pub fn insert_char(&mut self, ch: char) {
        let coalesce = if ch.is_whitespace() { None } else { Some(b'i') };
        let auto_close = self.auto_close;
        self.record_tx(coalesce, |doc| {
            let close_pair = matching_close(ch);
            // Apply edits from last cursor to first so earlier byte offsets stay valid!
            let mut new_cursors = doc.cursors.clone();

            for i in (0..doc.cursors.len()).rev() {
                let c = doc.cursors[i];
                let (s, e) = c.range();

                // 1. Wrap active selection when typing an opening delimiter (VS Code behavior!)
                if auto_close && !c.is_empty() {
                    if let Some(closing) = close_pair {
                        let selected = doc.rope.slice(s, e);
                        let mut wrapped = String::with_capacity(selected.len() + 2);
                        wrapped.push(ch);
                        wrapped.push_str(&selected);
                        wrapped.push(closing);
                        doc.rope.remove(s, e);
                        doc.rope.insert(s, &wrapped);
                        let delta = wrapped.len() as isize - (e - s) as isize;
                        shift_tail(&mut new_cursors[(i + 1)..], delta);
                        new_cursors[i] = Selection {
                            anchor: s + ch.len_utf8(),
                            head: s + ch.len_utf8() + selected.len(),
                            desired_col: 0,
                        };
                        continue;
                    }
                }

                // 2. Skip over an already-present closing delimiter
                if auto_close && c.is_empty() && matches!(ch, ')' | ']' | '}' | '"' | '\'') {
                    let next = doc.rope.slice(s, (s + 1).min(doc.rope.len_bytes()));
                    if next.as_bytes().first() == Some(&(ch as u8)) {
                        new_cursors[i] = Selection::caret(s + 1);
                        continue;
                    }
                }

                // 3. Auto-close pair on empty cursor (only if next char is whitespace/delimiter/EOL)
                let should_pair = auto_close && c.is_empty() && close_pair.is_some() && {
                    let next = doc.rope.slice(s, (s + 1).min(doc.rope.len_bytes()));
                    let ok_next = next
                        .chars()
                        .next()
                        .map_or(true, |nc| nc.is_whitespace() || matches!(nc, ')' | ']' | '}' | ',' | ';' | ':'));
                    let ok_prev = if ch == '\'' || ch == '"' {
                        let prev = if s > 0 { doc.rope.slice(s - 1, s) } else { String::new() };
                        !prev.chars().next().is_some_and(|pc| pc.is_alphanumeric() || pc == '_')
                    } else {
                        true
                    };
                    ok_next && ok_prev
                };

                let insert_str = if should_pair {
                    let mut tmp = String::with_capacity(2);
                    tmp.push(ch);
                    tmp.push(close_pair.unwrap());
                    tmp
                } else {
                    ch.to_string()
                };

                if s < e {
                    doc.rope.remove(s, e);
                }
                doc.rope.insert(s, &insert_str);
                let delta = insert_str.len() as isize - (e - s) as isize;
                shift_tail(&mut new_cursors[(i + 1)..], delta);
                let caret = s + ch.len_utf8();
                new_cursors[i] = Selection::caret(caret);
            }

            doc.cursors = new_cursors;
        });
    }

    pub fn insert_newline(&mut self) {
        let indent_w = self.tab_width;
        self.record_tx(None, |doc| {
            let mut new_cursors = doc.cursors.clone();
            for i in (0..doc.cursors.len()).rev() {
                let (s, e) = doc.cursors[i].range();
                let (row, _) = doc.rope.byte_to_line_col(s);
                let line_txt = doc.rope.line(row);
                let lead_ws: String = line_txt.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
                let before = doc.rope.slice(doc.rope.line_start_byte(row), s);
                let after = doc.rope.slice(e, doc.rope.line_end_byte(row));
                let prev_non_ws = before.trim_end().chars().last();
                let next_non_ws = after.trim_start().chars().next();

                let extra = matches!(prev_non_ws, Some('{' | '[' | '(' | ':'));
                let split_pair = matches!(
                    (prev_non_ws, next_non_ws),
                    (Some('{'), Some('}')) | (Some('['), Some(']')) | (Some('('), Some(')'))
                );

                let ins_indent = if extra {
                    format!("{}{}", lead_ws, " ".repeat(indent_w))
                } else {
                    lead_ws.clone()
                };

                let (insert_str, caret_offset) = if split_pair {
                    let s1 = format!("\n{}", ins_indent);
                    let s2 = format!("\n{}", lead_ws);
                    let off = s1.len();
                    (format!("{}{}", s1, s2), off)
                } else {
                    let s1 = format!("\n{}", ins_indent);
                    let off = s1.len();
                    (s1, off)
                };

                if s < e {
                    doc.rope.remove(s, e);
                }
                doc.rope.insert(s, &insert_str);
                shift_tail(&mut new_cursors[(i + 1)..], insert_str.len() as isize - (e - s) as isize);
                new_cursors[i] = Selection::caret(s + caret_offset);
            }
            doc.cursors = new_cursors;
        });
    }

    /// Shift+Enter: insert new line below without breaking current line.
    pub fn insert_line_below(&mut self) {
        self.record_tx(None, |doc| {
            let mut new_cursors = doc.cursors.clone();
            for i in (0..doc.cursors.len()).rev() {
                let (row, _) = doc.rope.byte_to_line_col(doc.cursors[i].head);
                let eol = doc.rope.line_end_byte(row);
                let line_txt = doc.rope.line(row);
                let lead_ws: String = line_txt.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
                let ins = format!("\n{}", lead_ws);
                doc.rope.insert(eol, &ins);
                shift_tail(&mut new_cursors[(i + 1)..], ins.len() as isize);
                new_cursors[i] = Selection::caret(eol + ins.len());
            }
            doc.cursors = new_cursors;
        });
    }

    pub fn paste(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let lines: Vec<&str> = text.split('\n').collect();
        let distribute = self.cursors.len() > 1 && lines.len() == self.cursors.len();
        self.record_tx(None, |doc| {
            let mut new_cursors = doc.cursors.clone();
            for i in (0..doc.cursors.len()).rev() {
                let piece = if distribute { lines[i] } else { text };
                let (s, e) = doc.cursors[i].range();
                if s < e {
                    doc.rope.remove(s, e);
                }
                doc.rope.insert(s, piece);
                shift_tail(&mut new_cursors[(i + 1)..], piece.len() as isize - (e - s) as isize);
                new_cursors[i] = Selection::caret(s + piece.len());
            }
            doc.cursors = new_cursors;
        });
    }

    pub fn backspace(&mut self, word: bool) {
        let coalesce = if word { None } else { Some(b'b') };
        let auto_close = self.auto_close;
        self.record_tx(coalesce, |doc| {
            let mut new_cursors = doc.cursors.clone();
            for i in (0..doc.cursors.len()).rev() {
                let c = doc.cursors[i];
                let (mut s, mut e) = c.range();
                if s == e {
                    if s == 0 {
                        continue;
                    }
                    if word {
                        s = prev_subword_byte(&doc.rope, s);
                    } else {
                        // Delete matching empty pair `(|)` in one keystroke
                        if auto_close && s < doc.rope.len_bytes() {
                            let prev_c = doc.rope.slice(prev_char_byte(&doc.rope, s), s);
                            let next_c = doc.rope.slice(s, next_char_byte(&doc.rope, s));
                            if let (Some(pc), Some(nc)) = (prev_c.chars().next(), next_c.chars().next()) {
                                if matching_close(pc) == Some(nc) {
                                    s = prev_char_byte(&doc.rope, s);
                                    e = next_char_byte(&doc.rope, e);
                                } else {
                                    s = prev_char_byte(&doc.rope, s);
                                }
                            } else {
                                s = prev_char_byte(&doc.rope, s);
                            }
                        } else {
                            s = prev_char_byte(&doc.rope, s);
                        }
                    }
                }
                if s < e {
                    doc.rope.remove(s, e);
                    shift_tail(&mut new_cursors[(i + 1)..], -((e - s) as isize));
                    new_cursors[i] = Selection::caret(s);
                }
            }
            doc.cursors = new_cursors;
        });
    }

    pub fn delete(&mut self, word: bool) {
        let coalesce = if word { None } else { Some(b'd') };
        self.record_tx(coalesce, |doc| {
            let mut new_cursors = doc.cursors.clone();
            for i in (0..doc.cursors.len()).rev() {
                let (s, mut e) = doc.cursors[i].range();
                if s == e {
                    if e >= doc.rope.len_bytes() {
                        continue;
                    }
                    e = if word {
                        next_subword_byte(&doc.rope, e)
                    } else {
                        next_char_byte(&doc.rope, e)
                    };
                }
                if s < e {
                    doc.rope.remove(s, e);
                    shift_tail(&mut new_cursors[(i + 1)..], -((e - s) as isize));
                    new_cursors[i] = Selection::caret(s);
                }
            }
            doc.cursors = new_cursors;
        });
    }

    pub fn indent(&mut self, unindent: bool) {
        let w = self.tab_width;
        let spaces = " ".repeat(w);
        let has_multiline_or_sel = self.cursors.iter().any(|c| !c.is_empty());

        if !unindent && !has_multiline_or_sel {
            self.record_tx(None, |doc| {
                let mut new_cursors = doc.cursors.clone();
                for i in (0..doc.cursors.len()).rev() {
                    let s = doc.cursors[i].head;
                    doc.rope.insert(s, &spaces);
                    shift_tail(&mut new_cursors[(i + 1)..], spaces.len() as isize);
                    new_cursors[i] = Selection::caret(s + spaces.len());
                }
                doc.cursors = new_cursors;
            });
            return;
        }

        let rows = self.affected_rows();
        self.record_tx(None, |doc| {
            let mut deltas = Vec::new();
            for &row in rows.iter().rev() {
                let ls = doc.rope.line_start_byte(row);
                if unindent {
                    let txt = doc.rope.line(row);
                    let rem = if txt.starts_with('\t') {
                        1
                    } else {
                        txt.bytes().take(w).take_while(|&b| b == b' ').count()
                    };
                    if rem > 0 {
                        doc.rope.remove(ls, ls + rem);
                        deltas.push((ls, -(rem as isize)));
                    }
                } else {
                    doc.rope.insert(ls, &spaces);
                    deltas.push((ls, spaces.len() as isize));
                }
            }
            let mut new_cursors = doc.cursors.clone();
            apply_shifts(&mut new_cursors, &deltas);
            doc.cursors = new_cursors;
        });
    }

    pub fn delete_lines(&mut self) {
        let rows = self.affected_rows();
        self.record_tx(None, |doc| {
            for &row in rows.iter().rev() {
                let s = doc.rope.line_start_byte(row);
                let e = if row + 1 < doc.rope.line_count() {
                    doc.rope.line_start_byte(row + 1)
                } else {
                    doc.rope.len_bytes()
                };
                if row + 1 >= doc.rope.line_count() && s > 0 {
                    doc.rope.remove(s - 1, e);
                } else {
                    doc.rope.remove(s, e);
                }
            }
            let first = rows.first().copied().unwrap_or(0).min(doc.rope.line_count().saturating_sub(1));
            doc.cursors = vec![Selection::caret(doc.rope.line_start_byte(first))];
            doc.primary = 0;
        });
    }

    pub fn duplicate(&mut self, up: bool) {
        let has_sel = self.cursors.iter().any(|c| !c.is_empty());
        if has_sel {
            self.record_tx(None, |doc| {
                let mut new_cursors = doc.cursors.clone();
                for i in (0..doc.cursors.len()).rev() {
                    let (s, e) = doc.cursors[i].range();
                    let txt = doc.rope.slice(s, e);
                    doc.rope.insert(e, &txt);
                    shift_tail(&mut new_cursors[(i + 1)..], txt.len() as isize);
                    new_cursors[i] = Selection {
                        anchor: e,
                        head: e + txt.len(),
                        desired_col: 0,
                    };
                }
                doc.cursors = new_cursors;
            });
        } else {
            let rows = self.affected_rows();
            self.record_tx(None, |doc| {
                let first = *rows.first().unwrap_or(&0);
                let last = *rows.last().unwrap_or(&0);
                let block_s = doc.rope.line_start_byte(first);
                let block_e = doc.rope.line_end_byte(last);
                let block = doc.rope.slice(block_s, block_e);
                let ins = format!("\n{}", block);
                doc.rope.insert(block_e, &ins);
                if !up {
                    let shift = ins.len();
                    for c in &mut doc.cursors {
                        c.anchor += shift;
                        c.head += shift;
                    }
                }
            });
        }
    }

    pub fn move_lines(&mut self, up: bool) {
        let rows = self.affected_rows();
        let Some(&first) = rows.first() else { return };
        let Some(&last) = rows.last() else { return };
        if up && first == 0 {
            return;
        }
        if !up && last + 1 >= self.rope.line_count() {
            return;
        }
        self.record_tx(None, |doc| {
            let block_s = doc.rope.line_start_byte(first);
            let block_e = doc.rope.line_end_byte(last);
            let block = doc.rope.slice(block_s, block_e);
            if up {
                let prev_s = doc.rope.line_start_byte(first - 1);
                let prev_line = doc.rope.line(first - 1);
                let replacement = format!("{}\n{}", block, prev_line);
                doc.rope.remove(prev_s, block_e);
                doc.rope.insert(prev_s, &replacement);
                let shift = prev_line.len() + 1;
                for c in &mut doc.cursors {
                    c.anchor = c.anchor.saturating_sub(shift);
                    c.head = c.head.saturating_sub(shift);
                }
            } else {
                let next_e = doc.rope.line_end_byte(last + 1);
                let next_line = doc.rope.line(last + 1);
                let replacement = format!("{}\n{}", next_line, block);
                doc.rope.remove(block_s, next_e);
                doc.rope.insert(block_s, &replacement);
                let shift = next_line.len() + 1;
                for c in &mut doc.cursors {
                    c.anchor += shift;
                    c.head += shift;
                }
            }
        });
    }

    pub fn toggle_comment(&mut self) {
        let prefix = self.lang.comment_prefix();
        let prefix_sp = format!("{} ", prefix);
        let rows = self.affected_rows();
        let all_commented = rows.iter().all(|&r| {
            let t = self.rope.line(r);
            let tr = t.trim_start();
            tr.is_empty() || tr.starts_with(prefix)
        });

        self.record_tx(None, |doc| {
            let mut deltas = Vec::new();
            for &row in rows.iter().rev() {
                let txt = doc.rope.line(row);
                let lead_bytes = txt.len() - txt.trim_start().len();
                let ls = doc.rope.line_start_byte(row) + lead_bytes;
                let trimmed = &txt[lead_bytes..];
                if trimmed.is_empty() {
                    continue;
                }
                if all_commented {
                    let rem = if trimmed.starts_with(&prefix_sp) {
                        prefix_sp.len()
                    } else if trimmed.starts_with(prefix) {
                        prefix.len()
                    } else {
                        0
                    };
                    if rem > 0 {
                        doc.rope.remove(ls, ls + rem);
                        deltas.push((ls, -(rem as isize)));
                    }
                } else {
                    doc.rope.insert(ls, &prefix_sp);
                    deltas.push((ls, prefix_sp.len() as isize));
                }
            }
            let mut new_cursors = doc.cursors.clone();
            apply_shifts(&mut new_cursors, &deltas);
            doc.cursors = new_cursors;
        });
    }

    pub fn trim_trailing_whitespace(&mut self) -> usize {
        let mut trimmed = 0usize;
        self.record_tx(None, |doc| {
            for row in (0..doc.rope.line_count()).rev() {
                let txt = doc.rope.line(row);
                let tr = txt.trim_end_matches(|c| c == ' ' || c == '\t');
                let diff = txt.len() - tr.len();
                if diff > 0 {
                    let end = doc.rope.line_end_byte(row);
                    doc.rope.remove(end - diff, end);
                    trimmed += diff;
                }
            }
        });
        trimmed
    }

    pub fn copy_text(&self) -> String {
        let mut parts = Vec::new();
        for c in &self.cursors {
            if c.is_empty() {
                let (row, _) = self.rope.byte_to_line_col(c.head);
                parts.push(self.rope.line(row));
            } else {
                let (s, e) = c.range();
                parts.push(self.rope.slice(s, e));
            }
        }
        parts.join("\n")
    }

    pub fn cut_text(&mut self) -> String {
        let txt = self.copy_text();
        let any_sel = self.cursors.iter().any(|c| !c.is_empty());
        if any_sel {
            self.record_tx(None, |doc| {
                let mut new_cursors = doc.cursors.clone();
                for i in (0..doc.cursors.len()).rev() {
                    let (s, e) = doc.cursors[i].range();
                    if s < e {
                        doc.rope.remove(s, e);
                        shift_tail(&mut new_cursors[(i + 1)..], -((e - s) as isize));
                        new_cursors[i] = Selection::caret(s);
                    }
                }
                doc.cursors = new_cursors;
            });
        } else {
            self.delete_lines();
        }
        txt
    }

    pub fn replace_active_match(&mut self) -> bool {
        let Some((s, e)) = self.search.active_match() else {
            return false;
        };
        let repl = self.search.replace.clone();
        self.record_tx(None, |doc| {
            doc.rope.remove(s, e);
            doc.rope.insert(s, &repl);
            doc.cursors = vec![Selection::caret(s + repl.len())];
            doc.primary = 0;
        });
        let cur = self.primary_cursor().head;
        self.search.recompute(&self.rope, cur);
        true
    }

    pub fn replace_all_matches(&mut self) -> usize {
        if self.search.matches.is_empty() {
            return 0;
        }
        let matches = self.search.matches.clone();
        let repl = self.search.replace.clone();
        let count = matches.len();
        self.record_tx(None, |doc| {
            for &(s, e) in matches.iter().rev() {
                doc.rope.remove(s, e);
                doc.rope.insert(s, &repl);
            }
            let last_s = matches[0].0 + repl.len();
            doc.cursors = vec![Selection::caret(last_s.min(doc.rope.len_bytes()))];
            doc.primary = 0;
        });
        let cur = self.primary_cursor().head;
        self.search.recompute(&self.rope, cur);
        count
    }

    pub fn word_at_primary(&self) -> Option<String> {
        let p = self.primary_cursor();
        if !p.is_empty() {
            return None;
        }
        let (s, e) = self.word_bounds_at(p.head)?;
        let w = self.rope.slice(s, e);
        if w.chars().count() < 2 || !w.chars().any(|c| c.is_alphabetic()) || !w.chars().all(is_word_char) {
            return None;
        }
        Some(w)
    }

    /// Vim-style `Alt+B` line bookmark toggle (`◆` in gutter).
    pub fn toggle_bookmark(&mut self) -> (bool, usize) {
        let (row, _) = self.primary_line_col();
        let added = if self.bookmarks.contains(&row) {
            self.bookmarks.remove(&row);
            false
        } else {
            self.bookmarks.insert(row);
            true
        };
        (added, row)
    }

    /// Jump to next/previous bookmarked line (wraps around).
    pub fn jump_bookmark(&mut self, forward: bool) -> Option<(usize, usize, usize)> {
        let max_r = self.rope.line_count().saturating_sub(1);
        self.bookmarks.retain(|&r| r <= max_r);
        if self.bookmarks.is_empty() {
            return None;
        }
        let (cur_row, _) = self.primary_line_col();
        let list: Vec<usize> = self.bookmarks.iter().copied().collect();
        let idx = if forward {
            list.iter().position(|&r| r > cur_row).unwrap_or(0)
        } else {
            list.iter().rposition(|&r| r < cur_row).unwrap_or(list.len() - 1)
        };
        let target = list[idx];
        self.goto_line(target + 1);
        Some((idx + 1, list.len(), target + 1))
    }

    /// Jump to next/previous modified Git hunk (`Alt+]` / `Alt+[`, like Vim `]c` / `[c`).
    pub fn jump_git_hunk(&mut self, forward: bool) -> Option<(usize, usize, usize)> {
        let marks = self.git.snapshot();
        if marks.is_empty() {
            return None;
        }
        let mut rows: Vec<usize> = marks.keys().copied().collect();
        rows.sort_unstable();
        // Collapse contiguous marked rows into distinct hunk start lines
        let mut hunks = Vec::new();
        for r in rows {
            if hunks.last().map_or(true, |&prev: &usize| r > prev + 1) {
                hunks.push(r);
            }
        }
        if hunks.is_empty() {
            return None;
        }
        let (cur_row, _) = self.primary_line_col();
        let idx = if forward {
            hunks.iter().position(|&r| r > cur_row).unwrap_or(0)
        } else {
            hunks.iter().rposition(|&r| r < cur_row).unwrap_or(hunks.len() - 1)
        };
        let target = hunks[idx];
        self.goto_line(target + 1);
        Some((idx + 1, hunks.len(), target + 1))
    }

    /// Smart Vim-like `Alt+I` Expand Selection:
    /// word -> inner `() [] {} "" '' `` ` -> around delimiters -> outer block -> line -> document.
    pub fn expand_selection(&mut self) {
        let p = self.primary_cursor();
        let (s, e) = p.range();
        if s == e {
            if let Some((ws, we)) = self.word_bounds_at(s) {
                self.cursors = vec![Selection {
                    anchor: ws,
                    head: we,
                    desired_col: 0,
                }];
                self.primary = 0;
                return;
            }
        }

        // Look for the tightest enclosing pair around (s, e) within a 32 KB window
        const WIN: usize = 32_768;
        let win_start = s.saturating_sub(WIN);
        let win_end = (e + WIN).min(self.rope.len_bytes());
        let chunk = self.rope.slice(win_start, win_end);
        let rel_s = s - win_start;
        let rel_e = e - win_start;

        if let Some((ns, ne)) = find_enclosing_expansion(&chunk, rel_s, rel_e) {
            self.cursors = vec![Selection {
                anchor: win_start + ns,
                head: win_start + ne,
                desired_col: 0,
            }];
            self.primary = 0;
            return;
        }

        // Fallback: expand to current line, then full document
        let (row, _) = self.rope.byte_to_line_col(s);
        let ls = self.rope.line_start_byte(row);
        let le = self.rope.line_end_byte(row);
        if s > ls || e < le {
            self.cursors = vec![Selection {
                anchor: ls,
                head: le,
                desired_col: 0,
            }];
            self.primary = 0;
        } else {
            self.select_all();
        }
    }

    fn affected_rows(&self) -> Vec<usize> {
        let mut set = BTreeSet::new();
        for c in &self.cursors {
            let (s, e) = c.range();
            let (sr, _) = self.rope.byte_to_line_col(s);
            let (mut er, ec) = self.rope.byte_to_line_col(e);
            if e > s && ec == 0 && er > sr {
                er -= 1;
            }
            for r in sr..=er {
                set.insert(r);
            }
        }
        set.into_iter().collect()
    }

    fn word_bounds_at(&self, byte_pos: usize) -> Option<(usize, usize)> {
        let (row, col) = self.rope.byte_to_line_col(byte_pos);
        let line = self.rope.line(row);
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        if chars.is_empty() {
            return None;
        }
        let mut idx = col.min(chars.len());
        if idx == chars.len() || !is_word_char(chars[idx].1) {
            if idx > 0 && is_word_char(chars[idx - 1].1) {
                idx -= 1;
            } else if idx == chars.len() {
                idx -= 1;
            }
        }
        let target_is_word = is_word_char(chars[idx].1);
        let mut s = idx;
        let mut e = idx;
        while s > 0 && is_word_char(chars[s - 1].1) == target_is_word && !chars[s - 1].1.is_whitespace() {
            s -= 1;
        }
        while e + 1 < chars.len()
            && is_word_char(chars[e + 1].1) == target_is_word
            && !chars[e + 1].1.is_whitespace()
        {
            e += 1;
        }
        if chars[s].1.is_whitespace() {
            return None;
        }
        let ls = self.rope.line_start_byte(row);
        let sb = ls + chars[s].0;
        let eb = ls + chars[e].0 + chars[e].1.len_utf8();
        Some((sb, eb))
    }
}

fn find_enclosing_expansion(chunk: &str, sel_s: usize, sel_e: usize) -> Option<(usize, usize)> {
    let bytes = chunk.as_bytes();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let mut stack: Vec<(u8, usize)> = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        // Handle quoted strings on the same line or backticks
        if matches!(b, b'"' | b'\'' | b'`') {
            let start = i;
            i += 1;
            let mut found_end = None;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if bytes[i] == b {
                    found_end = Some(i);
                    i += 1;
                    break;
                }
                if b != b'`' && bytes[i] == b'\n' {
                    break;
                }
                i += 1;
            }
            if let Some(end) = found_end {
                pairs.push((start, end));
            }
            continue;
        }
        match b {
            b'(' | b'[' | b'{' => stack.push((b, i)),
            b')' | b']' | b'}' => {
                let want = match b {
                    b')' => b'(',
                    b']' => b'[',
                    _ => b'{',
                };
                if let Some(pos) = stack.iter().rposition(|&(op, _)| op == want) {
                    let (_, open_i) = stack.remove(pos);
                    pairs.push((open_i, i));
                }
            }
            _ => {}
        }
        i += 1;
    }

    // Choose the smallest enclosing pair that strictly expands (sel_s, sel_e)
    let mut best: Option<(usize, usize)> = None;
    for (open_i, close_i) in pairs {
        if open_i > sel_s || close_i + 1 < sel_e {
            continue;
        }
        let inner = (open_i + 1, close_i);
        let around = (open_i, close_i + 1);
        let candidate = if sel_s >= inner.0 && sel_e <= inner.1 && (sel_s, sel_e) != inner && inner.0 < inner.1 {
            inner
        } else if (sel_s, sel_e) != around {
            around
        } else {
            continue;
        };
        let cand_len = candidate.1 - candidate.0;
        if best.map_or(true, |(bs, be)| cand_len < (be - bs)) {
            best = Some(candidate);
        }
    }
    best
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn matching_close(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        _ => None,
    }
}

fn shift_tail(tail: &mut [Selection], delta: isize) {
    for c in tail.iter_mut() {
        c.anchor = (c.anchor as isize + delta).max(0) as usize;
        c.head = (c.head as isize + delta).max(0) as usize;
    }
}

/// Apply line-start edits `(edit_pos, delta)` (from indent/comment) to existing cursors.
fn apply_shifts(cursors: &mut [Selection], deltas: &[(usize, isize)]) {
    for &(pos, delta) in deltas {
        for c in cursors.iter_mut() {
            if c.anchor >= pos {
                c.anchor = (c.anchor as isize + delta).max(pos as isize) as usize;
            }
            if c.head >= pos {
                c.head = (c.head as isize + delta).max(pos as isize) as usize;
            }
        }
    }
}

fn prev_char_byte(rope: &Rope, byte_pos: usize) -> usize {
    if byte_pos == 0 {
        return 0;
    }
    let start = byte_pos.saturating_sub(4);
    let chunk = rope.slice(start, byte_pos);
    let last_len = chunk.chars().next_back().map(|c| c.len_utf8()).unwrap_or(1);
    byte_pos.saturating_sub(last_len)
}

fn next_char_byte(rope: &Rope, byte_pos: usize) -> usize {
    let len = rope.len_bytes();
    if byte_pos >= len {
        return len;
    }
    let end = (byte_pos + 4).min(len);
    let chunk = rope.slice(byte_pos, end);
    let first_len = chunk.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    (byte_pos + first_len).min(len)
}

fn char_class(c: char) -> u8 {
    if c.is_alphanumeric() {
        1
    } else if c.is_whitespace() {
        0
    } else {
        2
    }
}

fn prev_subword_byte(rope: &Rope, byte_pos: usize) -> usize {
    if byte_pos == 0 {
        return 0;
    }
    let mut p = prev_char_byte(rope, byte_pos);
    let c0 = rope.slice(p, byte_pos).chars().next().unwrap_or(' ');
    if c0 == '\n' {
        return p;
    }
    // Skip horizontal spaces first
    if c0.is_whitespace() {
        while p > 0 {
            let np = prev_char_byte(rope, p);
            let ch = rope.slice(np, p).chars().next().unwrap_or(' ');
            if ch == '\n' || !ch.is_whitespace() {
                break;
            }
            p = np;
        }
        if p == 0 {
            return 0;
        }
    }
    let np = prev_char_byte(rope, p);
    let target_ch = rope.slice(p, next_char_byte(rope, p)).chars().next().unwrap_or(' ');
    let cls = char_class(target_ch);
    let _ = np;
    while p > 0 {
        let prev = prev_char_byte(rope, p);
        let ch = rope.slice(prev, p).chars().next().unwrap_or(' ');
        if ch == '\n' || char_class(ch) != cls {
            break;
        }
        p = prev;
    }
    p
}

fn next_subword_byte(rope: &Rope, byte_pos: usize) -> usize {
    let len = rope.len_bytes();
    if byte_pos >= len {
        return len;
    }
    let mut p = byte_pos;
    let first = rope.slice(p, next_char_byte(rope, p)).chars().next().unwrap_or(' ');
    if first == '\n' {
        return next_char_byte(rope, p);
    }
    let cls = char_class(first);
    while p < len {
        let nxt = next_char_byte(rope, p);
        let ch = rope.slice(p, nxt).chars().next().unwrap_or(' ');
        if ch == '\n' || char_class(ch) != cls {
            break;
        }
        p = nxt;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multicursor_ctrl_d_and_atomic_undo() {
        let mut doc = Document::new_empty();
        doc.rope = Rope::from_str("let foo = 1;\nlet bar = foo + foo;\n");
        doc.cursors = vec![Selection::caret(4)]; // inside 'foo'

        doc.select_next_occurrence(); // select 1st 'foo'
        assert_eq!(doc.cursors.len(), 1);
        doc.select_next_occurrence(); // add 2nd 'foo'
        assert_eq!(doc.cursors.len(), 2);
        doc.select_next_occurrence(); // add 3rd 'foo'
        assert_eq!(doc.cursors.len(), 3);

        doc.paste("baz");
        assert_eq!(
            doc.rope.slice(0, doc.rope.len_bytes()),
            "let baz = 1;\nlet bar = baz + baz;\n"
        );

        // Single undo reverts all 3 cursors atomically!
        assert!(doc.undo());
        assert_eq!(
            doc.rope.slice(0, doc.rope.len_bytes()),
            "let foo = 1;\nlet bar = foo + foo;\n"
        );
        assert!(doc.redo());
        assert_eq!(
            doc.rope.slice(0, doc.rope.len_bytes()),
            "let baz = 1;\nlet bar = baz + baz;\n"
        );
    }

    #[test]
    fn wrap_selection_move_lines_duplicate_and_comment() {
        let mut doc = Document::new_empty();
        doc.lang = Language::Rust;
        doc.rope = Rope::from_str("alpha\nbeta\ngamma");
        doc.cursors = vec![Selection { anchor: 0, head: 5, desired_col: 0 }];
        doc.insert_char('"');
        assert_eq!(doc.rope.line(0), "\"alpha\"");

        doc.collapse_cursors();
        doc.move_lines(false); // move line 0 down
        assert_eq!(doc.rope.line(0), "beta");
        assert_eq!(doc.rope.line(1), "\"alpha\"");

        doc.toggle_comment();
        assert_eq!(doc.rope.line(1), "// \"alpha\"");
        doc.toggle_comment();
        assert_eq!(doc.rope.line(1), "\"alpha\"");

        doc.duplicate(false);
        assert_eq!(doc.rope.line_count(), 4);
        assert_eq!(doc.rope.line(1), "\"alpha\"");
        assert_eq!(doc.rope.line(2), "\"alpha\"");
    }

    #[test]
    fn ctrl_shift_l_and_replace_all() {
        let mut doc = Document::new_empty();
        doc.rope = Rope::from_str("cat dog cat bird cat");
        doc.cursors = vec![Selection::caret(1)];
        doc.select_all_occurrences();
        assert_eq!(doc.cursors.len(), 3);
        doc.paste("fox");
        assert_eq!(doc.rope.line(0), "fox dog fox bird fox");

        doc.search.query = "fox".into();
        doc.search.replace = "wolf".into();
        doc.search.recompute(&doc.rope, 0);
        assert_eq!(doc.replace_all_matches(), 3);
        assert_eq!(doc.rope.line(0), "wolf dog wolf bird wolf");
    }

    #[test]
    fn smart_expand_selection_and_bookmarks() {
        let mut doc = Document::new_empty();
        doc.rope = Rope::from_str("fn run(args: Vec<\"hello world\">) {\n    let x = 1;\n}\n");
        let pos = doc.rope.slice(0, doc.rope.len_bytes()).find("hello").unwrap() + 1;
        doc.cursors = vec![Selection::caret(pos)];

        // 1) word -> "hello"
        doc.expand_selection();
        let (s, e) = doc.primary_cursor().range();
        assert_eq!(doc.rope.slice(s, e), "hello");

        // 2) inner quotes -> "hello world"
        doc.expand_selection();
        let (s, e) = doc.primary_cursor().range();
        assert_eq!(doc.rope.slice(s, e), "hello world");

        // 3) around quotes -> "\"hello world\""
        doc.expand_selection();
        let (s, e) = doc.primary_cursor().range();
        assert_eq!(doc.rope.slice(s, e), "\"hello world\"");

        // 4) inner parens -> "args: Vec<\"hello world\">"
        doc.expand_selection();
        let (s, e) = doc.primary_cursor().range();
        assert_eq!(doc.rope.slice(s, e), "args: Vec<\"hello world\">");

        // Bookmarks toggle & wrap jump
        doc.goto_line(1);
        assert_eq!(doc.toggle_bookmark(), (true, 0));
        doc.goto_line(3);
        assert_eq!(doc.toggle_bookmark(), (true, 2));
        doc.goto_line(1);
        assert_eq!(doc.jump_bookmark(true), Some((2, 2, 3)));
        assert_eq!(doc.jump_bookmark(true), Some((1, 2, 1)));
    }
}
