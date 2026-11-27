mod config;
mod document;
mod git;
mod input;
mod layout;
mod palette;
mod picker;
mod render;
mod rope;
mod search;
mod syntax;
mod terminal;

use config::EditorConfig;
use document::{Document, Motion, Selection};
use input::{Event, InputDecoder, KeyCode, KeyEvent, MouseKind};
use layout::{
    ComputedLayout, Dir, DividerHit, LayoutNode, PaneId, Rect, SplitAxis, MIN_PANE_H, MIN_PANE_W,
};
use palette::Palette;
use picker::{Picker, PickerAction};
use render::{
    char_col_at_visual, ensure_cursor_visible, render_editor, DropPreview, DropZone, LayoutInfo,
    PaneRenderInput, PromptBar, TabBadge,
};
use rope::LineEnding;
use std::env;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use terminal::{copy_to_system_clipboard, paste_from_system_clipboard, Terminal};

const VERSION: &str = "nave-edit 0.2.0 (Rust Native · Multi-Pane · Nave Terminal v4)";

enum Mode {
    Edit,
    ChordK,
    ResizePane,
    Find,
    Replace { focus_replace: bool },
    GotoLine(String),
    SaveAs(String),
    ConfirmQuit,
    ConfirmCloseTab,
    Picker(Picker),
}

#[derive(Clone, Copy, Debug)]
enum DragState {
    None,
    Selection(PaneId),
    Divider(DividerHit),
    Tab {
        src_pane: PaneId,
        src_tab: usize,
        start_col: usize,
        start_row: usize,
        cur_col: usize,
        cur_row: usize,
        dragging: bool,
    },
}

#[derive(Clone, Debug)]
struct TabView {
    doc_id: usize,
    scroll_row: usize,
    scroll_col: usize,
    cursors: Vec<Selection>,
    primary: usize,
}

impl TabView {
    fn from_doc(doc_id: usize, doc: &Document) -> Self {
        Self {
            doc_id,
            scroll_row: doc.scroll_row,
            scroll_col: doc.scroll_col,
            cursors: doc.cursors.clone(),
            primary: doc.primary,
        }
    }
}

#[derive(Clone, Debug)]
struct Pane {
    id: PaneId,
    tabs: Vec<TabView>,
    active_tab: usize,
}

impl Pane {
    fn active_view(&self) -> &TabView {
        &self.tabs[self.active_tab]
    }

    fn active_view_mut(&mut self) -> &mut TabView {
        &mut self.tabs[self.active_tab]
    }
}

struct App {
    docs: Vec<Option<Document>>,
    panes: Vec<Pane>,
    layout_tree: LayoutNode,
    active_pane: PaneId,
    next_pane_id: PaneId,
    next_split_id: usize,
    palette: &'static Palette,
    cfg: EditorConfig,
    term_sync_needed: bool,
    mode: Mode,
    clipboard: String,
    status_msg: Option<(String, Instant)>,
    layout: LayoutInfo,
    last_computed: ComputedLayout,
    drag: DragState,
    last_click: Option<(Instant, usize, usize, u8)>,
    should_quit: bool,
    last_mtime_check: Instant,
}

impl App {
    fn new(
        mut initial_docs: Vec<Document>,
        initial_split: Option<SplitAxis>,
        palette: &'static Palette,
        cfg: EditorConfig,
    ) -> Self {
        if initial_docs.is_empty() {
            initial_docs.push(Document::new_empty());
        }
        for d in &mut initial_docs {
            d.auto_close = cfg.auto_close;
        }

        let mut docs: Vec<Option<Document>> = Vec::with_capacity(initial_docs.len());
        let mut panes: Vec<Pane> = Vec::new();
        let mut layout_tree = LayoutNode::leaf(0);
        let mut next_pane_id = 1usize;
        let mut next_split_id = 1usize;

        if let Some(axis) = initial_split.filter(|_| initial_docs.len() >= 2) {
            for (idx, d) in initial_docs.into_iter().enumerate() {
                let tv = TabView::from_doc(idx, &d);
                docs.push(Some(d));
                if idx == 0 {
                    panes.push(Pane { id: 0, tabs: vec![tv], active_tab: 0 });
                } else {
                    let pid = next_pane_id;
                    next_pane_id += 1;
                    let sid = next_split_id;
                    next_split_id += 1;
                    let prev_pid = panes.last().map(|p| p.id).unwrap_or(0);
                    panes.push(Pane { id: pid, tabs: vec![tv], active_tab: 0 });
                    layout_tree.split_pane(prev_pid, pid, sid, axis, false);
                }
            }
            layout_tree.equalize_all();
        } else {
            let mut tabs = Vec::with_capacity(initial_docs.len());
            for (idx, d) in initial_docs.into_iter().enumerate() {
                tabs.push(TabView::from_doc(idx, &d));
                docs.push(Some(d));
            }
            panes.push(Pane { id: 0, tabs, active_tab: 0 });
        }

        Self {
            docs,
            panes,
            layout_tree,
            active_pane: 0,
            next_pane_id,
            next_split_id,
            palette,
            cfg,
            term_sync_needed: false,
            mode: Mode::Edit,
            clipboard: String::new(),
            status_msg: None,
            layout: LayoutInfo::default(),
            last_computed: ComputedLayout::default(),
            drag: DragState::None,
            last_click: None,
            should_quit: false,
            last_mtime_check: Instant::now(),
        }
    }

    fn alloc_doc(&mut self, mut doc: Document) -> usize {
        doc.auto_close = self.cfg.auto_close;
        if let Some(slot) = self.docs.iter().position(|d| d.is_none()) {
            self.docs[slot] = Some(doc);
            slot
        } else {
            let id = self.docs.len();
            self.docs.push(Some(doc));
            id
        }
    }

    fn pane(&self, id: PaneId) -> &Pane {
        self.panes
            .iter()
            .find(|p| p.id == id)
            .unwrap_or(&self.panes[0])
    }

    fn pane_mut(&mut self, id: PaneId) -> &mut Pane {
        let idx = self
            .panes
            .iter()
            .position(|p| p.id == id)
            .unwrap_or(0);
        &mut self.panes[idx]
    }

    fn active_doc_id(&self) -> usize {
        self.pane(self.active_pane).active_view().doc_id
    }

    fn doc(&self) -> &Document {
        let id = self.active_doc_id();
        self.docs[id].as_ref().expect("active document must exist")
    }

    fn doc_mut(&mut self) -> &mut Document {
        let id = self.active_doc_id();
        self.docs[id].as_mut().expect("active document must exist")
    }

    /// Save live cursor/scroll from `docs[active_doc_id]` back into the active `TabView`.
    fn sync_active_to_tab(&mut self) {
        let doc_id = self.active_doc_id();
        if let Some(doc) = self.docs.get(doc_id).and_then(|d| d.as_ref()) {
            let scroll_row = doc.scroll_row;
            let scroll_col = doc.scroll_col;
            let cursors = doc.cursors.clone();
            let primary = doc.primary;
            let tv = self.pane_mut(self.active_pane).active_view_mut();
            tv.scroll_row = scroll_row;
            tv.scroll_col = scroll_col;
            tv.cursors = cursors;
            tv.primary = primary;
        }
    }

    /// Load the active `TabView`'s cursor/scroll into `docs[active_doc_id]`, clamping to valid UTF-8 bounds.
    fn load_active_from_tab(&mut self) {
        let tv = self.pane(self.active_pane).active_view().clone();
        let doc_id = tv.doc_id;
        if let Some(doc) = self.docs.get_mut(doc_id).and_then(|d| d.as_mut()) {
            let mut cursors = tv.cursors;
            let mut primary = tv.primary;
            let mut scroll_row = tv.scroll_row;
            doc.clamp_view_state(&mut cursors, &mut primary, &mut scroll_row);
            doc.cursors = cursors.clone();
            doc.primary = primary;
            doc.scroll_row = scroll_row;
            doc.scroll_col = tv.scroll_col;
            let cur_tv = self.pane_mut(self.active_pane).active_view_mut();
            cur_tv.cursors = cursors;
            cur_tv.primary = primary;
            cur_tv.scroll_row = scroll_row;
        }
    }

    fn focus_pane_and_tab(&mut self, pane_id: PaneId, tab_idx: usize) {
        if !self.panes.iter().any(|p| p.id == pane_id) {
            return;
        }
        self.sync_active_to_tab();
        self.active_pane = pane_id;
        let pane = self.pane_mut(pane_id);
        pane.active_tab = tab_idx.min(pane.tabs.len().saturating_sub(1));
        self.load_active_from_tab();
    }

    fn focus_pane(&mut self, pane_id: PaneId) {
        if pane_id == self.active_pane || !self.panes.iter().any(|p| p.id == pane_id) {
            return;
        }
        let tab_idx = self.pane(pane_id).active_tab;
        self.focus_pane_and_tab(pane_id, tab_idx);
    }

    fn doc_ref_count(&self, doc_id: usize) -> usize {
        self.panes
            .iter()
            .flat_map(|p| p.tabs.iter())
            .filter(|t| t.doc_id == doc_id)
            .count()
    }

    fn pane_area(&self, cols: usize, rows: usize) -> Rect {
        let has_prompt = matches!(
            self.mode,
            Mode::Find
                | Mode::Replace { .. }
                | Mode::GotoLine(_)
                | Mode::SaveAs(_)
                | Mode::ConfirmQuit
                | Mode::ConfirmCloseTab
        );
        let bottom_reserved = if has_prompt { 2 } else { 1 };
        Rect {
            x: 0,
            y: 0,
            w: cols,
            h: rows.saturating_sub(bottom_reserved).max(2),
        }
    }

    fn group_number_of(&self, pane_id: PaneId) -> usize {
        self.layout_tree
            .pane_order()
            .iter()
            .position(|&id| id == pane_id)
            .map(|i| i + 1)
            .unwrap_or(1)
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

        // 1. Already open in active pane?
        let existing_in_pane = self.pane(self.active_pane).tabs.iter().position(|tv| {
            self.docs
                .get(tv.doc_id)
                .and_then(|d| d.as_ref())
                .and_then(|d| d.path.as_ref())
                .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()) == canon)
                .unwrap_or(false)
        });
        if let Some(tidx) = existing_in_pane {
            self.focus_pane_and_tab(self.active_pane, tidx);
            if let Some(ln) = jump_line {
                self.doc_mut().goto_line(ln);
            }
            return;
        }

        // 2. Already loaded in another pane? Share buffer live!
        let existing_doc = self.docs.iter().enumerate().find_map(|(id, slot)| {
            let d = slot.as_ref()?;
            let p = d.path.as_ref()?;
            if p.canonicalize().unwrap_or_else(|_| p.clone()) == canon {
                Some(id)
            } else {
                None
            }
        });

        let doc_id = if let Some(id) = existing_doc {
            id
        } else {
            match Document::open(path, jump_line) {
                Ok(doc) => self.alloc_doc(doc),
                Err(e) => {
                    self.set_status(format!("Erro ao abrir: {}", e));
                    return;
                }
            }
        };

        self.sync_active_to_tab();
        let new_tv = {
            let d = self.docs[doc_id].as_ref().unwrap();
            TabView::from_doc(doc_id, d)
        };

        // Replace untouched initial blank tab in active pane if present
        let replace_blank = {
            let p = self.pane(self.active_pane);
            if p.tabs.len() == 1 {
                let old_id = p.tabs[0].doc_id;
                if let Some(old_doc) = self.docs[old_id].as_ref() {
                    old_doc.path.is_none()
                        && !old_doc.is_dirty()
                        && old_doc.rope.is_empty()
                        && self.doc_ref_count(old_id) == 1
                } else {
                    false
                }
            } else {
                false
            }
        };

        if replace_blank {
            let old_id = self.pane(self.active_pane).tabs[0].doc_id;
            let p = self.pane_mut(self.active_pane);
            p.tabs[0] = new_tv;
            p.active_tab = 0;
            if old_id != doc_id && self.doc_ref_count(old_id) == 0 {
                self.docs[old_id] = None;
            }
        } else {
            let p = self.pane_mut(self.active_pane);
            p.tabs.push(new_tv);
            p.active_tab = p.tabs.len() - 1;
        }
        self.load_active_from_tab();
        if let Some(ln) = jump_line {
            self.doc_mut().goto_line(ln);
        }
        let name = self.doc().display_name();
        self.set_status(format!("Aberto: {}", name));
    }

    // -------------------------------------------------------------------------
    // Multi-Pane Operations (Split, Travel, Move, Focus, Resize, Swap, Close)
    // -------------------------------------------------------------------------

    fn travel_editor(&mut self, delta: isize) {
        self.sync_active_to_tab();
        let order = self.layout_tree.pane_order();
        let mut all: Vec<(PaneId, usize)> = Vec::new();
        for pid in &order {
            let p = self.pane(*pid);
            for ti in 0..p.tabs.len() {
                all.push((*pid, ti));
            }
        }
        if all.len() <= 1 {
            return;
        }
        let cur_tab = self.pane(self.active_pane).active_tab;
        let cur_pos = all
            .iter()
            .position(|&(pid, ti)| pid == self.active_pane && ti == cur_tab)
            .unwrap_or(0);
        let next_pos = ((cur_pos as isize + delta).rem_euclid(all.len() as isize)) as usize;
        let (next_pid, next_ti) = all[next_pos];
        self.active_pane = next_pid;
        self.pane_mut(next_pid).active_tab = next_ti;
        self.load_active_from_tab();
    }

    fn cycle_tab_in_active_pane(&mut self, delta: isize) {
        let n = self.pane(self.active_pane).tabs.len();
        if n <= 1 {
            return;
        }
        self.sync_active_to_tab();
        let p = self.pane_mut(self.active_pane);
        p.active_tab = ((p.active_tab as isize + delta).rem_euclid(n as isize)) as usize;
        self.load_active_from_tab();
    }

    fn reorder_active_tab(&mut self, delta: isize) {
        let n = self.pane(self.active_pane).tabs.len();
        if n <= 1 {
            return;
        }
        self.sync_active_to_tab();
        let p = self.pane_mut(self.active_pane);
        let cur = p.active_tab;
        let target = ((cur as isize + delta).clamp(0, (n - 1) as isize)) as usize;
        if target != cur {
            p.tabs.swap(cur, target);
            p.active_tab = target;
            self.set_status(format!("Aba movida para posicao {}/{}", target + 1, n));
        }
    }

    fn split_active(&mut self, axis: SplitAxis, place_first: bool, cols: usize, rows: usize) {
        let area = self.pane_area(cols, rows);
        let computed = self.layout_tree.compute(area);
        if let Some(r) = computed.rect_of(self.active_pane) {
            match axis {
                SplitAxis::Horizontal if r.w < MIN_PANE_W * 2 + 1 => {
                    self.set_status("Janela estreita demais para dividir lado a lado");
                    return;
                }
                SplitAxis::Vertical if r.h < MIN_PANE_H * 2 + 1 => {
                    self.set_status("Janela baixa demais para dividir acima/abaixo");
                    return;
                }
                _ => {}
            }
        }

        self.sync_active_to_tab();
        let cloned_tv = self.pane(self.active_pane).active_view().clone();
        let old_pid = self.active_pane;
        let new_pid = self.next_pane_id;
        self.next_pane_id += 1;
        let split_id = self.next_split_id;
        self.next_split_id += 1;

        self.panes.push(Pane {
            id: new_pid,
            tabs: vec![cloned_tv],
            active_tab: 0,
        });
        self.layout_tree
            .split_pane(old_pid, new_pid, split_id, axis, place_first);
        self.active_pane = new_pid;
        self.load_active_from_tab();

        let lbl = match axis {
            SplitAxis::Horizontal => "lado a lado (│)",
            SplitAxis::Vertical => "acima/abaixo (─)",
        };
        let num = self.group_number_of(new_pid);
        self.set_status(format!("Painel dividido {} → Painel {}", lbl, num));
    }

    fn focus_direction(&mut self, dir: Dir, cols: usize, rows: usize) {
        let computed = self.layout_tree.compute(self.pane_area(cols, rows));
        if let Some(target) = layout::find_neighbor(self.active_pane, dir, &computed) {
            self.focus_pane(target);
            let num = self.group_number_of(target);
            self.set_status(format!("Foco: Painel {}", num));
        }
    }

    fn focus_pane_by_number(&mut self, one_based: usize) {
        let order = self.layout_tree.pane_order();
        if let Some(&pid) = order.get(one_based.saturating_sub(1)) {
            self.focus_pane(pid);
            self.set_status(format!("Painel {}/{}", one_based, order.len()));
        }
    }

    fn move_active_tab(&mut self, dir: Dir, cols: usize, rows: usize) {
        let area = self.pane_area(cols, rows);
        let computed = self.layout_tree.compute(area);
        self.sync_active_to_tab();

        // Case 1: Existing neighbor pane in `dir` -> move tab into it
        if let Some(neighbor_id) = layout::find_neighbor(self.active_pane, dir, &computed) {
            let src_id = self.active_pane;
            let moved_tv = {
                let src = self.pane_mut(src_id);
                let tv = src.tabs.remove(src.active_tab);
                if !src.tabs.is_empty() && src.active_tab >= src.tabs.len() {
                    src.active_tab = src.tabs.len() - 1;
                }
                tv
            };
            let src_empty = self.pane(src_id).tabs.is_empty();
            if src_empty {
                self.panes.retain(|p| p.id != src_id);
                self.layout_tree.remove_pane(src_id);
            }

            let dst = self.pane_mut(neighbor_id);
            if let Some(existing_idx) = dst.tabs.iter().position(|t| t.doc_id == moved_tv.doc_id) {
                dst.tabs[existing_idx] = moved_tv;
                dst.active_tab = existing_idx;
            } else {
                dst.tabs.push(moved_tv);
                dst.active_tab = dst.tabs.len() - 1;
            }
            self.active_pane = neighbor_id;
            self.load_active_from_tab();
            let num = self.group_number_of(neighbor_id);
            self.set_status(format!("Aba movida para Painel {}", num));
            return;
        }

        // Case 2: No neighbor in `dir`
        let axis = match dir {
            Dir::Left | Dir::Right => SplitAxis::Horizontal,
            Dir::Up | Dir::Down => SplitAxis::Vertical,
        };
        let place_first = matches!(dir, Dir::Left | Dir::Up);
        let src_tabs_len = self.pane(self.active_pane).tabs.len();

        if src_tabs_len > 1 {
            // Detach active tab into a new split in direction `dir`
            if let Some(r) = computed.rect_of(self.active_pane) {
                let fits = match axis {
                    SplitAxis::Horizontal => r.w >= MIN_PANE_W * 2 + 1,
                    SplitAxis::Vertical => r.h >= MIN_PANE_H * 2 + 1,
                };
                if !fits {
                    self.set_status("Sem espaco suficiente para nova divisao nesta direcao");
                    return;
                }
            }
            let src_id = self.active_pane;
            let moved_tv = {
                let src = self.pane_mut(src_id);
                let tv = src.tabs.remove(src.active_tab);
                if src.active_tab >= src.tabs.len() {
                    src.active_tab = src.tabs.len() - 1;
                }
                tv
            };
            let new_pid = self.next_pane_id;
            self.next_pane_id += 1;
            let split_id = self.next_split_id;
            self.next_split_id += 1;
            self.panes.push(Pane {
                id: new_pid,
                tabs: vec![moved_tv],
                active_tab: 0,
            });
            self.layout_tree
                .split_pane(src_id, new_pid, split_id, axis, place_first);
            self.active_pane = new_pid;
            self.load_active_from_tab();
            let lbl = match axis {
                SplitAxis::Horizontal => "divisao lateral (│)",
                SplitAxis::Vertical => "divisao horizontal (─)",
            };
            self.set_status(format!("Aba movida para nova {}", lbl));
        } else if self.panes.len() >= 2 {
            // Re-dock this single-tab pane onto the requested edge of the layout tree!
            let src_id = self.active_pane;
            if self.layout_tree.remove_pane(src_id).is_some() {
                let old_root = std::mem::replace(&mut self.layout_tree, LayoutNode::leaf(0));
                let moved = Box::new(LayoutNode::leaf(src_id));
                let rest = Box::new(old_root);
                let (first, second) = if place_first { (moved, rest) } else { (rest, moved) };
                let split_id = self.next_split_id;
                self.next_split_id += 1;
                self.layout_tree = LayoutNode::Split {
                    id: split_id,
                    axis,
                    ratio: 0.5,
                    first,
                    second,
                };
                self.load_active_from_tab();
                let lbl = match dir {
                    Dir::Left => "esquerda (│)",
                    Dir::Right => "direita (│)",
                    Dir::Up => "topo (─)",
                    Dir::Down => "base (─)",
                };
                self.set_status(format!("Painel movido para {}", lbl));
            }
        } else {
            // Single pane, single tab -> mirror-split in requested direction (VS Code behavior)
            self.split_active(axis, place_first, cols, rows);
        }
    }

    fn toggle_split_orientation(&mut self) {
        if let Some(new_axis) = self.layout_tree.toggle_axis(self.active_pane) {
            let lbl = match new_axis {
                SplitAxis::Horizontal => "Lado a Lado (│)",
                SplitAxis::Vertical => "Acima / Abaixo (─)",
            };
            self.set_status(format!("Orientacao da divisao: {}", lbl));
        } else {
            self.set_status("Divida a janela primeiro (Ctrl+\\ ou Ctrl+K -)");
        }
    }

    fn swap_active_pane(&mut self) {
        if self.layout_tree.swap_sibling(self.active_pane) {
            self.set_status("Paineis trocados de lugar");
        } else {
            self.set_status("Apenas 1 painel aberto");
        }
    }

    fn resize_active_grow(&mut self, delta: f32) {
        if self.layout_tree.resize_pane_grow(self.active_pane, delta) {
            let dir = if delta > 0.0 { "aumentado" } else { "reduzido" };
            self.set_status(format!("Painel {}", dir));
        } else {
            self.set_status("Apenas 1 painel aberto");
        }
    }

    fn equalize_panes(&mut self) {
        if self.panes.len() > 1 {
            self.layout_tree.equalize_all();
            self.set_status("Paineis igualados (50/50)");
        }
    }

    fn close_active_pane(&mut self) {
        if self.panes.len() <= 1 {
            if self.doc().is_dirty() {
                self.mode = Mode::ConfirmCloseTab;
            } else {
                self.close_active_tab_force();
            }
            return;
        }
        self.sync_active_to_tab();
        let pid = self.active_pane;
        // Guard against dropping an exclusively-referenced dirty buffer
        let dirty_idx = self.pane(pid).tabs.iter().enumerate().find_map(|(ti, tv)| {
            let dirty = self.docs[tv.doc_id].as_ref().is_some_and(|d| d.is_dirty());
            if dirty && self.doc_ref_count(tv.doc_id) == 1 {
                Some(ti)
            } else {
                None
            }
        });
        if let Some(ti) = dirty_idx {
            self.focus_pane_and_tab(pid, ti);
            self.mode = Mode::ConfirmCloseTab;
            return;
        }

        let removed_doc_ids: Vec<usize> = self.pane(pid).tabs.iter().map(|t| t.doc_id).collect();
        self.panes.retain(|p| p.id != pid);
        for did in removed_doc_ids {
            if self.doc_ref_count(did) == 0 {
                self.docs[did] = None;
            }
        }
        if let Some(next_pid) = self.layout_tree.remove_pane(pid) {
            self.active_pane = next_pid;
            self.load_active_from_tab();
            self.set_status("Painel fechado");
        }
    }

    // -------------------------------------------------------------------------
    // Event Dispatch
    // -------------------------------------------------------------------------

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
            Mode::ConfirmQuit | Mode::ConfirmCloseTab | Mode::ChordK | Mode::ResizePane => {}
        }
    }

    fn handle_mouse(&mut self, m: input::MouseEvent, cols: usize, rows: usize) {
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
            MouseKind::WheelUp | MouseKind::WheelDown => {
                if self.cfg.keyboard_only_nav {
                    return;
                }
                let target_pid = self
                    .layout
                    .panes
                    .iter()
                    .find(|pl| pl.rect.contains(m.col, m.row))
                    .map(|pl| pl.pane_id)
                    .unwrap_or(self.active_pane);
                if target_pid == self.active_pane {
                    if m.kind == MouseKind::WheelUp {
                        self.doc_mut().scroll_row = self.doc().scroll_row.saturating_sub(3);
                    } else {
                        let max_r = self.doc().rope.line_count().saturating_sub(1);
                        self.doc_mut().scroll_row = (self.doc().scroll_row + 3).min(max_r);
                    }
                } else {
                    let doc_id = self.pane(target_pid).active_view().doc_id;
                    let max_r = self.docs[doc_id]
                        .as_ref()
                        .map(|d| d.rope.line_count().saturating_sub(1))
                        .unwrap_or(0);
                    let tv = self.pane_mut(target_pid).active_view_mut();
                    if m.kind == MouseKind::WheelUp {
                        tv.scroll_row = tv.scroll_row.saturating_sub(3);
                    } else {
                        tv.scroll_row = (tv.scroll_row + 3).min(max_r);
                    }
                }
            }
            MouseKind::LeftDown => {
                // 1. Check split dividers first for live drag-resizing (generous grab zone!)
                let has_sb = self.cfg.scrollbar;
                if let Some(div) = self
                    .layout
                    .dividers
                    .iter()
                    .copied()
                    .find(|d| d.hits(m.col, m.row, has_sb))
                {
                    self.drag = DragState::Divider(div);
                    self.mode = Mode::Edit;
                    self.set_status("Arraste para redimensionar a divisoria");
                    return;
                }

                // 2. Check pane tab bars -> focus/switch immediately AND arm DragState::Tab for drag-and-drop!
                let hit_tab = self.layout.panes.iter().find_map(|pl| {
                    if m.row == pl.tab_bar_row && pl.rect.contains(m.col, m.row) {
                        let tab_idx = pl
                            .tab_hitboxes
                            .iter()
                            .find(|&&(sc, ec, _)| m.col >= sc && m.col < ec)
                            .map(|&(_, _, ti)| ti);
                        Some((pl.pane_id, tab_idx))
                    } else {
                        None
                    }
                });
                if let Some((pid, opt_tidx)) = hit_tab {
                    self.mode = Mode::Edit;
                    if let Some(tidx) = opt_tidx {
                        self.focus_pane_and_tab(pid, tidx);
                        self.drag = DragState::Tab {
                            src_pane: pid,
                            src_tab: tidx,
                            start_col: m.col,
                            start_row: m.row,
                            cur_col: m.col,
                            cur_row: m.row,
                            dragging: false,
                        };
                    } else {
                        self.focus_pane(pid);
                        self.drag = DragState::None;
                    }
                    return;
                }

                let hit_pane = self
                    .layout
                    .panes
                    .iter()
                    .find(|pl| pl.rect.contains(m.col, m.row))
                    .map(|pl| pl.pane_id);
                if let Some(pid) = hit_pane {
                    self.focus_pane(pid);
                    self.mode = Mode::Edit;
                    if self.cfg.keyboard_only_nav {
                        // Keyboard-only text navigation: allow focusing another pane by click
                        // without hijacking or moving the text cursor inside the file
                        self.drag = DragState::None;
                        return;
                    }
                    if let Some((doc_row, doc_col)) = self.screen_to_doc_in_pane(pid, m.col, m.row) {
                        let now = Instant::now();
                        let clicks = if let Some((prev_t, pr, pc, cnt)) = self.last_click {
                            if now.duration_since(prev_t).as_millis() < 400
                                && pr == m.row
                                && pc.abs_diff(m.col) <= 1
                            {
                                (cnt % 3) + 1
                            } else {
                                1
                            }
                        } else {
                            1
                        };
                        self.last_click = Some((now, m.row, m.col, clicks));

                        match clicks {
                            2 => {
                                self.doc_mut().select_word_at_line_col(doc_row, doc_col);
                                self.drag = DragState::None;
                            }
                            3 => {
                                self.doc_mut().select_full_line_at(doc_row);
                                self.drag = DragState::None;
                            }
                            _ => {
                                self.doc_mut().set_cursor_at_line_col(doc_row, doc_col, m.shift);
                                self.drag = DragState::Selection(pid);
                            }
                        }
                    }
                }
            }
            MouseKind::LeftDrag => match self.drag {
                DragState::Divider(div) => {
                    let raw_ratio = match div.axis {
                        SplitAxis::Horizontal => {
                            let avail = div.parent_rect.w.saturating_sub(1).max(1);
                            m.col.saturating_sub(div.parent_rect.x) as f32 / avail as f32
                        }
                        SplitAxis::Vertical => {
                            let avail = div.parent_rect.h.saturating_sub(1).max(1);
                            m.row.saturating_sub(div.parent_rect.y) as f32 / avail as f32
                        }
                    };
                    let clamped = raw_ratio.clamp(0.15, 0.85);
                    self.layout_tree.set_split_ratio(div.split_id, clamped);
                    let pct1 = (clamped * 100.0).round() as usize;
                    let pct2 = 100usize.saturating_sub(pct1);
                    let sep = if div.axis == SplitAxis::Horizontal { "│" } else { "─" };
                    self.set_status(format!("Redimensionando ({}% {} {}%)", pct1, sep, pct2));
                }
                DragState::Tab {
                    src_pane,
                    src_tab,
                    start_col,
                    start_row,
                    mut dragging,
                    ..
                } => {
                    if !dragging && (m.col.abs_diff(start_col) >= 2 || m.row != start_row) {
                        dragging = true;
                    }
                    self.drag = DragState::Tab {
                        src_pane,
                        src_tab,
                        start_col,
                        start_row,
                        cur_col: m.col,
                        cur_row: m.row,
                        dragging,
                    };
                }
                DragState::Selection(pid) => {
                    if pid == self.active_pane {
                        if let Some((doc_row, doc_col)) = self.screen_to_doc_in_pane(pid, m.col, m.row) {
                            self.doc_mut().set_cursor_at_line_col(doc_row, doc_col, true);
                        }
                    }
                }
                DragState::None => {}
            },
            MouseKind::LeftUp => {
                if let DragState::Tab {
                    src_pane,
                    src_tab,
                    dragging: true,
                    ..
                } = self.drag
                {
                    if let Some(dp) = self.compute_drop_preview(src_pane, src_tab, m.col, m.row) {
                        self.apply_tab_drop(src_pane, src_tab, dp, cols, rows);
                    }
                }
                self.drag = DragState::None;
            }
        }
    }

    fn compute_drop_preview(
        &self,
        src_pane: PaneId,
        src_tab: usize,
        col: usize,
        row: usize,
    ) -> Option<DropPreview> {
        let src_p = self.panes.iter().find(|p| p.id == src_pane)?;
        let tv = src_p.tabs.get(src_tab)?;
        let file_name = self
            .docs
            .get(tv.doc_id)
            .and_then(|d| d.as_ref())
            .map(|d| d.display_name())
            .unwrap_or_else(|| "[Sem nome]".into());

        // Find target pane whose rect contains `(col, row)` (or nearest pane if over divider)
        let pl = self
            .layout
            .panes
            .iter()
            .find(|p| p.rect.contains(col, row))
            .or_else(|| {
                self.layout.panes.iter().min_by_key(|p| {
                    let dx = if col < p.rect.x {
                        p.rect.x - col
                    } else if col >= p.rect.x + p.rect.w {
                        col - (p.rect.x + p.rect.w)
                    } else {
                        0
                    };
                    let dy = if row < p.rect.y {
                        p.rect.y - row
                    } else if row >= p.rect.y + p.rect.h {
                        row - (p.rect.y + p.rect.h)
                    } else {
                        0
                    };
                    dx + dy
                })
            })?;

        let r = pl.rect;

        // 1. Over target pane's tab bar -> reorder / insert into tab bar!
        if row == pl.tab_bar_row {
            let target_p = self.panes.iter().find(|p| p.id == pl.pane_id)?;
            let mut insert_idx = target_p.tabs.len();
            let mut marker_col = pl
                .tab_hitboxes
                .last()
                .map(|&(_, ec, _)| ec)
                .unwrap_or(r.x + 1);
            for &(sc, ec, ti) in &pl.tab_hitboxes {
                let mid = (sc + ec) / 2;
                if col < mid {
                    insert_idx = ti;
                    marker_col = sc;
                    break;
                }
            }
            let label = if pl.pane_id == src_pane {
                "Reordenar aba"
            } else {
                "Mover para barra de abas"
            };
            return Some(DropPreview {
                target_pane: pl.pane_id,
                zone: DropZone::TabBar(insert_idx),
                preview_rect: Rect { x: r.x, y: r.y, w: r.w, h: 1 },
                tab_insert_col: Some(marker_col),
                file_name,
                label,
            });
        }

        // 2. Over target pane's editor canvas -> 5-zone VS Code split/center detection!
        let canvas_y = r.y + 1;
        let canvas_h = r.h.saturating_sub(1).max(1);
        let rel_x = col.saturating_sub(r.x) as f32 / r.w.max(1) as f32;
        let rel_y = row.saturating_sub(canvas_y) as f32 / canvas_h as f32;

        let d_left = rel_x;
        let d_right = (1.0 - rel_x).max(0.0);
        let d_top = rel_y;
        let d_bot = (1.0 - rel_y).max(0.0);

        let edge_x = d_left.min(d_right);
        let edge_y = d_top.min(d_bot);

        let zone = if edge_x < 0.28 || edge_y < 0.28 {
            if edge_x <= edge_y {
                if d_left <= d_right { DropZone::Left } else { DropZone::Right }
            } else if d_top <= d_bot {
                DropZone::Top
            } else {
                DropZone::Bottom
            }
        } else {
            DropZone::Center
        };

        let half_w = (r.w / 2).max(4);
        let half_h = (r.h / 2).max(3);
        let (preview_rect, label) = match zone {
            DropZone::Left => (
                Rect { x: r.x, y: r.y, w: half_w, h: r.h },
                "Dividir à Esquerda (│)",
            ),
            DropZone::Right => (
                Rect { x: r.x + r.w - half_w, y: r.y, w: half_w, h: r.h },
                "Dividir à Direita (│)",
            ),
            DropZone::Top => (
                Rect { x: r.x, y: r.y, w: r.w, h: half_h },
                "Dividir Acima (─)",
            ),
            DropZone::Bottom => (
                Rect { x: r.x, y: r.y + r.h - half_h, w: r.w, h: half_h },
                "Dividir Abaixo (─)",
            ),
            DropZone::Center => (r, "Mover para este Painel"),
            DropZone::TabBar(_) => unreachable!(),
        };

        Some(DropPreview {
            target_pane: pl.pane_id,
            zone,
            preview_rect,
            tab_insert_col: None,
            file_name,
            label,
        })
    }

    fn apply_tab_drop(
        &mut self,
        src_pane: PaneId,
        src_tab: usize,
        dp: DropPreview,
        cols: usize,
        rows: usize,
    ) {
        if !self.panes.iter().any(|p| p.id == src_pane)
            || src_tab >= self.pane(src_pane).tabs.len()
        {
            return;
        }
        self.sync_active_to_tab();

        match dp.zone {
            DropZone::TabBar(mut insert_idx) => {
                if dp.target_pane == src_pane {
                    // Reorder within the SAME pane
                    let p = self.pane_mut(src_pane);
                    let len = p.tabs.len();
                    if len <= 1 {
                        return;
                    }
                    let moved = p.tabs.remove(src_tab);
                    if insert_idx > src_tab {
                        insert_idx -= 1;
                    }
                    let idx = insert_idx.min(p.tabs.len());
                    p.tabs.insert(idx, moved);
                    p.active_tab = idx;
                    self.active_pane = src_pane;
                    self.load_active_from_tab();
                    if idx != src_tab {
                        self.set_status(format!("Aba reordenada para posicao {}/{}", idx + 1, len));
                    }
                } else {
                    // Move tab directly into target pane's tab bar at `insert_idx`
                    let moved = {
                        let src = self.pane_mut(src_pane);
                        let tv = src.tabs.remove(src_tab);
                        if !src.tabs.is_empty() && src.active_tab >= src.tabs.len() {
                            src.active_tab = src.tabs.len() - 1;
                        }
                        tv
                    };
                    if self.pane(src_pane).tabs.is_empty() {
                        self.panes.retain(|p| p.id != src_pane);
                        self.layout_tree.remove_pane(src_pane);
                    }
                    let dst = self.pane_mut(dp.target_pane);
                    if let Some(ex) = dst.tabs.iter().position(|t| t.doc_id == moved.doc_id) {
                        dst.tabs[ex] = moved;
                        dst.active_tab = ex;
                    } else {
                        let idx = insert_idx.min(dst.tabs.len());
                        dst.tabs.insert(idx, moved);
                        dst.active_tab = idx;
                    }
                    self.active_pane = dp.target_pane;
                    self.load_active_from_tab();
                    let num = self.group_number_of(dp.target_pane);
                    self.set_status(format!("Aba movida para Painel {}", num));
                }
            }
            DropZone::Center => {
                if dp.target_pane == src_pane {
                    return;
                }
                let moved = {
                    let src = self.pane_mut(src_pane);
                    let tv = src.tabs.remove(src_tab);
                    if !src.tabs.is_empty() && src.active_tab >= src.tabs.len() {
                        src.active_tab = src.tabs.len() - 1;
                    }
                    tv
                };
                if self.pane(src_pane).tabs.is_empty() {
                    self.panes.retain(|p| p.id != src_pane);
                    self.layout_tree.remove_pane(src_pane);
                }
                let dst = self.pane_mut(dp.target_pane);
                if let Some(ex) = dst.tabs.iter().position(|t| t.doc_id == moved.doc_id) {
                    dst.tabs[ex] = moved;
                    dst.active_tab = ex;
                } else {
                    dst.tabs.push(moved);
                    dst.active_tab = dst.tabs.len() - 1;
                }
                self.active_pane = dp.target_pane;
                self.load_active_from_tab();
                let num = self.group_number_of(dp.target_pane);
                self.set_status(format!("Aba movida para Painel {}", num));
            }
            DropZone::Left | DropZone::Right | DropZone::Top | DropZone::Bottom => {
                let axis = match dp.zone {
                    DropZone::Left | DropZone::Right => SplitAxis::Horizontal,
                    _ => SplitAxis::Vertical,
                };
                let place_first = matches!(dp.zone, DropZone::Left | DropZone::Top);
                let src_len = self.pane(src_pane).tabs.len();

                // Special case: dragging the ONLY tab of the ONLY pane onto its own edge -> mirror-split!
                if dp.target_pane == src_pane && src_len == 1 {
                    if self.panes.len() == 1 {
                        self.split_active(axis, place_first, cols, rows);
                        return;
                    }
                    // Dragging a 1-tab pane onto its OWN edge when other panes exist -> re-dock on outer edge
                    let dir = match dp.zone {
                        DropZone::Left => Dir::Left,
                        DropZone::Right => Dir::Right,
                        DropZone::Top => Dir::Up,
                        _ => Dir::Down,
                    };
                    self.active_pane = src_pane;
                    self.load_active_from_tab();
                    if self.layout_tree.remove_pane(src_pane).is_some() {
                        let old_root = std::mem::replace(&mut self.layout_tree, LayoutNode::leaf(0));
                        let moved = Box::new(LayoutNode::leaf(src_pane));
                        let rest = Box::new(old_root);
                        let (first, second) = if place_first { (moved, rest) } else { (rest, moved) };
                        let split_id = self.next_split_id;
                        self.next_split_id += 1;
                        self.layout_tree = LayoutNode::Split {
                            id: split_id,
                            axis,
                            ratio: 0.5,
                            first,
                            second,
                        };
                        let lbl = match dir {
                            Dir::Left => "esquerda (│)",
                            Dir::Right => "direita (│)",
                            Dir::Up => "topo (─)",
                            Dir::Down => "base (─)",
                        };
                        self.set_status(format!("Painel reposicionado na {}", lbl));
                    }
                    return;
                }

                // Check minimum pane size on target pane before splitting
                let area = self.pane_area(cols, rows);
                let computed = self.layout_tree.compute(area);
                if let Some(tr) = computed.rect_of(dp.target_pane) {
                    let fits = match axis {
                        SplitAxis::Horizontal => tr.w >= MIN_PANE_W * 2 + 1,
                        SplitAxis::Vertical => tr.h >= MIN_PANE_H * 2 + 1,
                    };
                    // Allow if collapsing `src_pane` will free space for target!
                    let will_collapse = src_len == 1 && dp.target_pane != src_pane;
                    if !fits && !will_collapse {
                        self.set_status("Espaco insuficiente para dividir este painel");
                        return;
                    }
                }

                // Detach tab from source pane (collapsing source pane if it becomes empty!)
                let moved_tv = {
                    let src = self.pane_mut(src_pane);
                    let tv = src.tabs.remove(src_tab);
                    if !src.tabs.is_empty() && src.active_tab >= src.tabs.len() {
                        src.active_tab = src.tabs.len() - 1;
                    }
                    tv
                };
                if self.pane(src_pane).tabs.is_empty() {
                    self.panes.retain(|p| p.id != src_pane);
                    self.layout_tree.remove_pane(src_pane);
                }

                // Split `dp.target_pane` with a new pane holding `moved_tv`
                let new_pid = self.next_pane_id;
                self.next_pane_id += 1;
                let split_id = self.next_split_id;
                self.next_split_id += 1;
                self.panes.push(Pane {
                    id: new_pid,
                    tabs: vec![moved_tv],
                    active_tab: 0,
                });
                self.layout_tree
                    .split_pane(dp.target_pane, new_pid, split_id, axis, place_first);
                self.active_pane = new_pid;
                self.load_active_from_tab();
                self.set_status(format!("{} → {}", dp.file_name, dp.label));
            }
        }
    }

    fn screen_to_doc_in_pane(&self, pid: PaneId, col: usize, row: usize) -> Option<(usize, usize)> {
        let pl = self.layout.panes.iter().find(|p| p.pane_id == pid)?;
        if row < pl.content_start_row {
            return None;
        }
        let rel_y = row - pl.content_start_row;
        let vinfo = pl.visual_rows.get(rel_y)?;
        let line_txt = self.doc().rope.line(vinfo.doc_row);
        let rel_x = col.saturating_sub(pl.rect.x + pl.gutter_width);

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

    fn handle_key(&mut self, k: KeyEvent, cols: usize, rows: usize) {
        // Global Alt+1..6 live palette switch (persisted in edit.conf)
        if k.alt && !k.ctrl && !k.shift {
            if let KeyCode::Char(c @ '1'..='6') = k.code {
                let idx = (c as u8 - b'1') as usize;
                self.set_palette_index(idx);
                return;
            }
        }

        match &mut self.mode {
            Mode::Picker(_) => {
                self.handle_picker_key(k, cols, rows);
                return;
            }
            Mode::ChordK => {
                self.handle_chord_k(k, cols, rows);
                return;
            }
            Mode::ResizePane => {
                self.handle_resize_mode(k);
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

        // ---- Main Edit Mode Keybindings (100% VS Code aligned + Multi-Pane) ----
        let page_step = self
            .last_computed
            .rect_of(self.active_pane)
            .map(|r| r.h.saturating_sub(2))
            .unwrap_or_else(|| rows.saturating_sub(4))
            .max(5);

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
                KeyCode::PageUp => {
                    self.reorder_active_tab(-1);
                    return;
                }
                KeyCode::PageDown => {
                    self.reorder_active_tab(1);
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

        // Ctrl+Alt combinations (VS Code multi-cursor vertical + move tab across panes!)
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
                KeyCode::Left => {
                    self.move_active_tab(Dir::Left, cols, rows);
                    return;
                }
                KeyCode::Right => {
                    self.move_active_tab(Dir::Right, cols, rows);
                    return;
                }
                _ => {}
            }
        }

        // Shift+Alt combinations (VS Code duplicate line up/down + Shift+Alt+0 flip split layout!)
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
                KeyCode::Char('0') | KeyCode::Char(')') => {
                    self.toggle_split_orientation();
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
                    if self.pane(self.active_pane).tabs.len() > 1 {
                        self.cycle_tab_in_active_pane(-1);
                    } else if self.panes.len() > 1 {
                        self.travel_editor(-1);
                    } else {
                        self.doc_mut().move_cursors(Motion::WordLeft, false);
                    }
                    return;
                }
                KeyCode::Right => {
                    if self.pane(self.active_pane).tabs.len() > 1 {
                        self.cycle_tab_in_active_pane(1);
                    } else if self.panes.len() > 1 {
                        self.travel_editor(1);
                    } else {
                        self.doc_mut().move_cursors(Motion::WordRight, false);
                    }
                    return;
                }
                KeyCode::Char('=') | KeyCode::Char('+') => {
                    self.resize_active_grow(0.05);
                    return;
                }
                KeyCode::Char('-') => {
                    self.resize_active_grow(-0.05);
                    return;
                }
                KeyCode::Char(')') => {
                    // VT Shift+Alt+0 sends ESC )
                    self.toggle_split_orientation();
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
                    let st = if self.cfg.vim_visual { "ON (relnum + nav teclado)" } else { "OFF (VS Code padrao)" };
                    self.set_status(format!("Estilo Vim Visual: {}", st));
                    return;
                }
                KeyCode::Char('m') => {
                    let head = self.doc().primary_cursor().head;
                    let at_cursor = self.cfg.block_cursor;
                    if let Some((_a, b)) = syntax::find_matching_bracket(&self.doc().rope, head, at_cursor) {
                        let doc = self.doc_mut();
                        let target = if at_cursor { b } else { (b + 1).min(doc.rope.len_bytes()) };
                        doc.cursors = vec![document::Selection::caret(target)];
                        doc.primary = 0;
                    }
                    return;
                }
                KeyCode::Char('i') => {
                    self.doc_mut().expand_selection();
                    return;
                }
                KeyCode::Char('b') => {
                    let (added, row) = self.doc_mut().toggle_bookmark();
                    if added {
                        self.set_status(format!("◆ Bookmark adicionado na linha {}", row + 1));
                    } else {
                        self.set_status(format!("Bookmark removido da linha {}", row + 1));
                    }
                    return;
                }
                KeyCode::Char(']') => {
                    if let Some((idx, tot, ln)) = self.doc_mut().jump_git_hunk(true) {
                        self.set_status(format!("Git hunk {}/{} (Ln {})", idx, tot, ln));
                    } else {
                        self.set_status("Nenhuma alteracao Git neste arquivo");
                    }
                    return;
                }
                KeyCode::Char('[') => {
                    if let Some((idx, tot, ln)) = self.doc_mut().jump_git_hunk(false) {
                        self.set_status(format!("Git hunk {}/{} (Ln {})", idx, tot, ln));
                    } else {
                        self.set_status("Nenhuma alteracao Git neste arquivo");
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
                KeyCode::PageUp => {
                    self.travel_editor(-1);
                    return;
                }
                KeyCode::PageDown => {
                    self.travel_editor(1);
                    return;
                }
                KeyCode::Char('\\') => {
                    self.split_active(SplitAxis::Horizontal, false, cols, rows);
                    return;
                }
                KeyCode::Char('k') => {
                    self.mode = Mode::ChordK;
                    return;
                }
                KeyCode::Char(c @ '1'..='9') => {
                    let num = (c as u8 - b'0') as usize;
                    self.focus_pane_by_number(num);
                    return;
                }
                KeyCode::Char('s') => {
                    self.try_save_active();
                    return;
                }
                KeyCode::Char('q') => {
                    if self.docs.iter().flatten().any(|d| d.is_dirty()) {
                        self.mode = Mode::ConfirmQuit;
                    } else {
                        self.should_quit = true;
                    }
                    return;
                }
                KeyCode::Char('w') => {
                    let did = self.active_doc_id();
                    if self.doc().is_dirty() && self.doc_ref_count(did) == 1 {
                        self.mode = Mode::ConfirmCloseTab;
                    } else {
                        self.close_active_tab_force();
                    }
                    return;
                }
                KeyCode::Char('n') => {
                    self.new_blank_tab_in_active_pane();
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
                if !matches!(self.drag, DragState::None) {
                    self.drag = DragState::None;
                    self.set_status("Arrasto cancelado");
                } else if !self.doc_mut().collapse_cursors() && !self.doc().search.query.is_empty() {
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
                self.set_palette_index(next_idx);
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
            KeyCode::F(4) => {
                if let Some((idx, tot, ln)) = self.doc_mut().jump_bookmark(!k.shift) {
                    self.set_status(format!("◆ Bookmark {}/{} (Ln {})", idx, tot, ln));
                } else {
                    self.set_status("Nenhum bookmark (use Alt+B para marcar linha)");
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

    fn handle_chord_k(&mut self, k: KeyEvent, cols: usize, rows: usize) {
        self.mode = Mode::Edit;
        match k.code {
            KeyCode::Esc => {}
            KeyCode::Char('\\') | KeyCode::Char('v') => {
                self.split_active(SplitAxis::Horizontal, false, cols, rows);
            }
            KeyCode::Char('-') | KeyCode::Char('h') => {
                self.split_active(SplitAxis::Vertical, false, cols, rows);
            }
            KeyCode::Char('o') | KeyCode::Char('0') => {
                self.toggle_split_orientation();
            }
            KeyCode::Char('s') => {
                self.swap_active_pane();
            }
            KeyCode::Char('=') => {
                self.equalize_panes();
            }
            KeyCode::Char('r') => {
                if self.panes.len() > 1 {
                    self.mode = Mode::ResizePane;
                } else {
                    self.set_status("Divida a janela primeiro (Ctrl+\\ ou Ctrl+K -)");
                }
            }
            KeyCode::Char('w') => {
                self.close_active_pane();
            }
            KeyCode::Left => {
                if k.shift {
                    self.move_active_tab(Dir::Left, cols, rows);
                } else {
                    self.focus_direction(Dir::Left, cols, rows);
                }
            }
            KeyCode::Right => {
                if k.shift {
                    self.move_active_tab(Dir::Right, cols, rows);
                } else {
                    self.focus_direction(Dir::Right, cols, rows);
                }
            }
            KeyCode::Up => {
                if k.shift {
                    self.move_active_tab(Dir::Up, cols, rows);
                } else {
                    self.focus_direction(Dir::Up, cols, rows);
                }
            }
            KeyCode::Down => {
                if k.shift {
                    self.move_active_tab(Dir::Down, cols, rows);
                } else {
                    self.focus_direction(Dir::Down, cols, rows);
                }
            }
            KeyCode::Char(c @ '1'..='9') => {
                self.focus_pane_by_number((c as u8 - b'0') as usize);
            }
            _ => {
                self.set_status("Ctrl+K cancelado");
            }
        }
    }

    fn handle_resize_mode(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                self.mode = Mode::Edit;
                self.set_status("Tamanho dos paineis confirmado");
            }
            KeyCode::Left => {
                self.layout_tree
                    .nudge_divider(self.active_pane, SplitAxis::Horizontal, -0.05);
            }
            KeyCode::Right => {
                self.layout_tree
                    .nudge_divider(self.active_pane, SplitAxis::Horizontal, 0.05);
            }
            KeyCode::Up => {
                self.layout_tree
                    .nudge_divider(self.active_pane, SplitAxis::Vertical, -0.05);
            }
            KeyCode::Down => {
                self.layout_tree
                    .nudge_divider(self.active_pane, SplitAxis::Vertical, 0.05);
            }
            KeyCode::Char('+') => {
                self.layout_tree.resize_pane_grow(self.active_pane, 0.05);
            }
            KeyCode::Char('-') => {
                self.layout_tree.resize_pane_grow(self.active_pane, -0.05);
            }
            KeyCode::Char('=') | KeyCode::Char('0') => {
                self.layout_tree.equalize_all();
            }
            _ => {}
        }
    }

    fn new_blank_tab_in_active_pane(&mut self) {
        self.sync_active_to_tab();
        let doc = Document::new_empty();
        let tv = TabView::from_doc(0, &doc);
        let id = self.alloc_doc(doc);
        let mut tv = tv;
        tv.doc_id = id;
        let p = self.pane_mut(self.active_pane);
        p.tabs.push(tv);
        p.active_tab = p.tabs.len() - 1;
        self.load_active_from_tab();
    }

    fn open_command_palette(&mut self) {
        let pk = Picker::command_palette(&self.cfg, self.doc().eol.label());
        self.mode = Mode::Picker(pk);
    }

    fn enter_find(&mut self, replace: bool) {
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
            KeyCode::F(4) => {
                if let Some((idx, tot, ln)) = self.doc_mut().jump_bookmark(!k.shift) {
                    self.set_status(format!("◆ Bookmark {}/{} (Ln {})", idx, tot, ln));
                } else {
                    self.set_status("Nenhum bookmark (use Alt+B para marcar linha)");
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
        match k.code {
            KeyCode::Esc => self.mode = Mode::Edit,
            KeyCode::Backspace => {
                if let Mode::GotoLine(ref mut s) = self.mode {
                    s.pop();
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() && !k.ctrl && !k.alt => {
                if let Mode::GotoLine(ref mut s) = self.mode {
                    s.push(c);
                }
            }
            KeyCode::Enter => {
                let num = if let Mode::GotoLine(ref s) = self.mode {
                    s.parse::<usize>().ok()
                } else {
                    None
                };
                self.mode = Mode::Edit;
                if let Some(ln) = num {
                    self.doc_mut().goto_line(ln);
                    self.set_status(format!("Linha {}", ln));
                }
            }
            _ => {}
        }
    }

    fn handle_save_as_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => self.mode = Mode::Edit,
            KeyCode::Backspace => {
                if let Mode::SaveAs(ref mut s) = self.mode {
                    s.pop();
                }
            }
            KeyCode::Char(c) if !k.ctrl && !k.alt => {
                if let Mode::SaveAs(ref mut s) = self.mode {
                    s.push(c);
                }
            }
            KeyCode::Enter => {
                let p = if let Mode::SaveAs(ref s) = self.mode {
                    s.trim().to_string()
                } else {
                    String::new()
                };
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

    fn handle_picker_key(&mut self, k: KeyEvent, cols: usize, rows: usize) {
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
                    let st = if self.cfg.vim_visual { "ON (relnum + nav teclado)" } else { "OFF (VS Code padrao)" };
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
                PickerAction::ToggleAutoHideMouse => {
                    self.cfg.auto_hide_mouse = !self.cfg.auto_hide_mouse;
                    self.cfg.save();
                    self.term_sync_needed = true;
                    let st = if self.cfg.auto_hide_mouse { "ON (sempre ativo)" } else { "OFF" };
                    self.set_status(format!("Auto-Hide Ponteiro do Mouse: {}", st));
                }
                PickerAction::ToggleKeyboardOnlyNav => {
                    self.cfg.keyboard_only_nav = !self.cfg.keyboard_only_nav;
                    self.cfg.save();
                    let st = if self.cfg.keyboard_only_nav {
                        "ON (texto so teclado · mouse p/ abas/divisorias)"
                    } else {
                        "OFF (mouse livre no texto)"
                    };
                    self.set_status(format!("Navegacao no Texto: {}", st));
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
                PickerAction::ToggleCursorWord => {
                    self.cfg.cursorword = !self.cfg.cursorword;
                    self.cfg.save();
                    let st = if self.cfg.cursorword { "ON" } else { "OFF" };
                    self.set_status(format!("Destaque de palavra sob cursor: {}", st));
                }
                PickerAction::ToggleIndentGuides => {
                    self.cfg.indent_guides = !self.cfg.indent_guides;
                    self.cfg.save();
                    let st = if self.cfg.indent_guides { "ON" } else { "OFF" };
                    self.set_status(format!("Guias de indentacao: {}", st));
                }
                PickerAction::ToggleRainbowBrackets => {
                    self.cfg.rainbow_brackets = !self.cfg.rainbow_brackets;
                    self.cfg.save();
                    let st = if self.cfg.rainbow_brackets { "ON (por nivel)" } else { "OFF (neutro)" };
                    self.set_status(format!("Cor de delimitadores: {}", st));
                }
                PickerAction::ToggleBracketMatch => {
                    self.cfg.bracket_match = !self.cfg.bracket_match;
                    self.cfg.save();
                    let st = if self.cfg.bracket_match { "ON" } else { "OFF" };
                    self.set_status(format!("Destaque de par de delimitadores: {}", st));
                }
                PickerAction::ToggleCommentBadges => {
                    self.cfg.comment_badges = !self.cfg.comment_badges;
                    self.cfg.save();
                    let st = if self.cfg.comment_badges { "ON" } else { "OFF" };
                    self.set_status(format!("Badges TODO/FIXME: {}", st));
                }
                PickerAction::ToggleTrailingSpaces => {
                    self.cfg.show_trailing = !self.cfg.show_trailing;
                    self.cfg.save();
                    let st = if self.cfg.show_trailing { "ON" } else { "OFF" };
                    self.set_status(format!("Espacos no fim da linha: {}", st));
                }
                PickerAction::CycleColorColumn => {
                    self.cfg.cycle_color_column();
                    let lbl = if self.cfg.color_column == 0 {
                        "OFF".to_string()
                    } else {
                        format!("coluna {}", self.cfg.color_column)
                    };
                    self.set_status(format!("Regua de coluna: {}", lbl));
                }
                PickerAction::ToggleWrap => {
                    self.cfg.wrap = !self.cfg.wrap;
                    self.cfg.save();
                    self.set_status(format!("Word Wrap: {}", if self.cfg.wrap { "ON" } else { "OFF" }));
                }
                PickerAction::ToggleAutoClose => {
                    self.cfg.auto_close = !self.cfg.auto_close;
                    self.cfg.save();
                    for d in self.docs.iter_mut().flatten() {
                        d.auto_close = self.cfg.auto_close;
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
                PickerAction::ReloadFromDisk => match self.doc_mut().reload_from_disk() {
                    Ok(()) => self.set_status("Recarregado do disco"),
                    Err(e) => self.set_status(format!("Erro: {}", e)),
                },
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
                PickerAction::NewBuffer => self.new_blank_tab_in_active_pane(),
                PickerAction::SplitHorizontal => {
                    self.split_active(SplitAxis::Horizontal, false, cols, rows);
                }
                PickerAction::SplitVertical => {
                    self.split_active(SplitAxis::Vertical, false, cols, rows);
                }
                PickerAction::ToggleSplitOrientation => self.toggle_split_orientation(),
                PickerAction::MoveTabRight => self.move_active_tab(Dir::Right, cols, rows),
                PickerAction::MoveTabLeft => self.move_active_tab(Dir::Left, cols, rows),
                PickerAction::MoveTabDown => self.move_active_tab(Dir::Down, cols, rows),
                PickerAction::MoveTabUp => self.move_active_tab(Dir::Up, cols, rows),
                PickerAction::SwapPanes => self.swap_active_pane(),
                PickerAction::ResizePaneMode => {
                    if self.panes.len() > 1 {
                        self.mode = Mode::ResizePane;
                    } else {
                        self.set_status("Divida a janela primeiro (Ctrl+\\ ou Ctrl+K -)");
                    }
                }
                PickerAction::EqualizePanes => self.equalize_panes(),
                PickerAction::ClosePane => self.close_active_pane(),
                PickerAction::SetPalette(idx) => self.set_palette_index(idx),
            }
        }
    }

    fn set_palette_index(&mut self, idx: usize) {
        self.palette = Palette::by_index(idx);
        self.cfg.palette = self.palette.name.to_string();
        self.cfg.save();
        self.term_sync_needed = true;
        self.set_status(format!("Paleta: {}", self.palette.name));
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
        for did in 0..self.docs.len() {
            let needs_save = self.docs[did].as_ref().is_some_and(|d| d.is_dirty());
            if !needs_save {
                continue;
            }
            let has_path = self.docs[did].as_ref().is_some_and(|d| d.path.is_some());
            if !has_path {
                // Focus a pane containing this unnamed dirty doc
                if let Some((pid, tidx)) = self.find_tab_for_doc(did) {
                    self.focus_pane_and_tab(pid, tidx);
                }
                self.mode = Mode::SaveAs(String::new());
                return false;
            }
            if let Some(doc) = self.docs[did].as_mut() {
                if let Err(e) = doc.save() {
                    if let Some((pid, tidx)) = self.find_tab_for_doc(did) {
                        self.focus_pane_and_tab(pid, tidx);
                    }
                    self.set_status(format!("Erro ao salvar: {}", e));
                    return false;
                }
            }
        }
        true
    }

    fn find_tab_for_doc(&self, doc_id: usize) -> Option<(PaneId, usize)> {
        for p in &self.panes {
            if let Some(ti) = p.tabs.iter().position(|t| t.doc_id == doc_id) {
                return Some((p.id, ti));
            }
        }
        None
    }

    fn close_active_tab_force(&mut self) {
        let pid = self.active_pane;
        let removed_did = {
            let p = self.pane_mut(pid);
            let tv = p.tabs.remove(p.active_tab);
            if !p.tabs.is_empty() && p.active_tab >= p.tabs.len() {
                p.active_tab = p.tabs.len() - 1;
            }
            tv.doc_id
        };
        if self.doc_ref_count(removed_did) == 0 {
            self.docs[removed_did] = None;
        }
        if self.pane(pid).tabs.is_empty() {
            if self.panes.len() <= 1 {
                self.should_quit = true;
            } else {
                self.panes.retain(|p| p.id != pid);
                if let Some(next_pid) = self.layout_tree.remove_pane(pid) {
                    self.active_pane = next_pid;
                    self.load_active_from_tab();
                }
            }
        } else {
            self.load_active_from_tab();
        }
    }

    fn render(&mut self, cols: usize, rows: usize, synced: bool) -> String {
        let area = self.pane_area(cols, rows);
        let computed = self.layout_tree.compute(area);
        let cfg = self.cfg.clone();

        // Keep active pane's cursor visible within its exact pane rectangle
        if let Some(active_rect) = computed.rect_of(self.active_pane) {
            let content_h = active_rect.h.saturating_sub(1).max(1);
            ensure_cursor_visible(self.doc_mut(), active_rect.w, content_h, &cfg);
        }
        self.sync_active_to_tab();

        // Build per-pane render inputs in deterministic computed order
        let pane_views: Vec<PaneRenderInput> = computed
            .panes
            .iter()
            .enumerate()
            .filter_map(|(order_idx, &(pid, rect))| {
                let p = self.panes.iter().find(|x| x.id == pid)?;
                let tv = p.active_view();
                let tabs = p
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(ti, tab)| {
                        let (name, dirty) = self
                            .docs
                            .get(tab.doc_id)
                            .and_then(|d| d.as_ref())
                            .map(|d| (d.display_name(), d.is_dirty()))
                            .unwrap_or_else(|| ("[Sem nome]".into(), false));
                        TabBadge {
                            name,
                            dirty,
                            active: ti == p.active_tab,
                        }
                    })
                    .collect();
                Some(PaneRenderInput {
                    pane_id: pid,
                    group_num: order_idx + 1,
                    rect,
                    is_focused: pid == self.active_pane,
                    doc_id: tv.doc_id,
                    tabs,
                    scroll_row: tv.scroll_row,
                    scroll_col: tv.scroll_col,
                    cursors: tv.cursors.clone(),
                    primary: tv.primary,
                })
            })
            .collect();

        let status = self.active_status().map(str::to_string);
        let drop_preview = match self.drag {
            DragState::Tab {
                src_pane,
                src_tab,
                cur_col,
                cur_row,
                dragging: true,
                ..
            } => self.compute_drop_preview(src_pane, src_tab, cur_col, cur_row),
            _ => None,
        };
        let drag_hint_owned = drop_preview.as_ref().map(|dp| {
            format!(
                "Arrastando {} → {} · Solte para posicionar · Esc cancela",
                dp.file_name, dp.label
            )
        });
        let mode_hint: Option<&str> = if let Some(ref dh) = drag_hint_owned {
            Some(dh.as_str())
        } else {
            match self.mode {
                Mode::ChordK => Some(
                    "Ctrl+K → [\\] lado a lado · [-] topo/base · [←↑↓→] focar · [Shift+seta] mover · [o] inverter · [s] trocar · [r] redimensionar · [=] 50/50 · [w] fechar",
                ),
                Mode::ResizePane => Some(
                    "REDIMENSIONAR → [←/→] largura · [↑/↓] altura · [+/-] painel · [=] 50/50 · [Enter/Esc] sair",
                ),
                _ => None,
            }
        };
        let search_snap = self.doc().search.clone();
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
            Mode::Edit | Mode::ChordK | Mode::ResizePane | Mode::Picker(_) => PromptBar::None,
        };
        let picker_ref = match &self.mode {
            Mode::Picker(pk) => Some(pk),
            _ => None,
        };
        let active_divider = match self.drag {
            DragState::Divider(d) => Some(d.split_id),
            _ => None,
        };
        let highlight_all_dividers = matches!(self.mode, Mode::ResizePane);

        let (frame, layout_info) = render_editor(
            &mut self.docs,
            &pane_views,
            &computed,
            active_divider,
            highlight_all_dividers,
            drop_preview.as_ref(),
            self.palette,
            cols,
            rows,
            &cfg,
            prompt,
            picker_ref,
            status.as_deref(),
            mode_hint,
            synced,
        );
        self.layout = layout_info;
        self.last_computed = computed;
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
        "{}\n\nUso:\n  nave-edit [opcoes] [arquivo[:linha] ...]\n  nave-edit +<linha> <arquivo>\n\nOpcoes:\n  -h, --help          Mostra esta ajuda\n  -v, --version       Mostra versao\n  -O                  Abre multiplos arquivos lado a lado (│)\n  -o                  Abre multiplos arquivos acima/abaixo (─)\n  --render-once <f>   Renderiza 1 frame estatico e sai\n\nAtalhos principais (VS Code + Multi-Janela):\n  Ctrl+S salvar · Ctrl+Q sair · Ctrl+W fechar aba · Ctrl+P abrir arquivo · F1 comandos\n  Ctrl+\\ dividir lado a lado · Ctrl+K - dividir topo/base · Ctrl+K o / Shift+Alt+0 inverter │/─\n  Ctrl+PgUp/PgDn viajar entre abas e paineis · Ctrl+1..4 focar painel · Ctrl+Alt+←/→ mover aba\n  Alt+=/Alt+- redimensionar painel · Ctrl+K r modo redimensionar 2D · Ctrl+K = igualar 50/50\n  Ctrl+D multi-cursor · Ctrl+Shift+L todas ocorrencias · Ctrl+F buscar · Alt+1..6 paletas",
        VERSION
    );
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut render_once = false;
    let mut initial_split: Option<SplitAxis> = None;
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
            "-O" => {
                initial_split = Some(SplitAxis::Horizontal);
            }
            "-o" => {
                initial_split = Some(SplitAxis::Vertical);
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

    let cfg = EditorConfig::load();
    let pal = Palette::load_for_editor(&cfg.palette);
    let mut app = App::new(docs, initial_split, pal, cfg);

    if render_once {
        let (cols, rows) = Terminal::size();
        let frame = app.render(cols, rows, false);
        print!("{}", frame);
        return;
    }

    let term = match Terminal::enter(app.cfg.block_cursor, app.palette.cursor_hex) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nave-edit: requer um terminal interativo (TTY): {}", e);
            std::process::exit(1);
        }
    };

    let mut mouse_hider = terminal::MouseHider::new();
    mouse_hider.sync(app.cfg.auto_hide_mouse);

    let mut decoder = InputDecoder::new();
    let mut read_buf = [0u8; 4096];
    let mut needs_redraw = true;
    let mut last_activity = Instant::now();
    let mut standby_blink = false;

    while !app.should_quit {
        let (cols, rows) = Terminal::size();

        if app.term_sync_needed {
            term.set_cursor_style(app.cfg.block_cursor, standby_blink);
            term.set_cursor_color(app.palette.cursor_hex);
            mouse_hider.sync(app.cfg.auto_hide_mouse);
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
        for d in app.docs.iter().flatten() {
            if d.git.take_dirty() {
                needs_redraw = true;
            }
        }
        if app.last_mtime_check.elapsed() >= Duration::from_secs(2) {
            for d in app.docs.iter_mut().flatten() {
                let prev = d.external_changed;
                d.check_external_change();
                if d.external_changed != prev {
                    needs_redraw = true;
                }
            }
            app.last_mtime_check = Instant::now();
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
                    if matches!(ev, Event::Key(_)) {
                        mouse_hider.notify_key();
                    }
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
