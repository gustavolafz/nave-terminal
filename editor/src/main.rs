mod config;
mod cp_assist;
mod document;
mod git;
mod input;
mod judge;
mod layout;
mod markdown;
mod palette;
mod picker;
mod pty_term;
mod render;
mod rope;
mod search;
mod syntax;
mod terminal;

use config::EditorConfig;
use cp_assist::{
    all_snippets, compute_completions, detect_cp_pitfall, extract_leetcode_solution,
    extract_symbols, file_has_custom_hash, find_snippet, CompletionKind, CompletionPopup, CpTimer,
};
use document::{Document, Motion, Selection};
use input::{Event, InputDecoder, KeyCode, KeyEvent, MouseKind};
use judge::{
    is_runnable_source, resolve_src_dir, scaffold_stress_files, DiagSeverity, Diagnostic,
    JudgeRunner,
};
use layout::{
    ComputedLayout, Dir, DividerHit, LayoutNode, PaneId, Rect, SplitAxis, MIN_PANE_H, MIN_PANE_W,
};
use palette::Palette;
use picker::{Picker, PickerAction};
use pty_term::IntegratedTerminal;
use render::{
    build_trail_map, char_col_at_visual, ensure_cursor_visible, pane_content_height,
    predict_primary_screen_pos, render_editor, CursorTrail, DropPreview, DropZone, LayoutInfo,
    PaneRenderInput, PromptBar, TabBadge, TermPanelView,
};
use rope::LineEnding;
use std::collections::{HashMap, VecDeque};
use std::env;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use terminal::{copy_to_system_clipboard, paste_from_system_clipboard, PointerShape, Terminal};

const VERSION: &str = "nave-edit 0.3.0 (Rust Native · Multi-Pane · PTY Terminal · CP Suite)";

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
    TermDivider,
    Scrollbar {
        pane_id: PaneId,
        start_row: usize,
        height: usize,
    },
    HScrollbar {
        pane_id: PaneId,
        start_col: usize,
        track_w: usize,
        max_scroll_col: usize,
    },
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
    md_preview: bool,
    md_scroll: usize,
}

impl TabView {
    fn from_doc(doc_id: usize, doc: &Document) -> Self {
        Self {
            doc_id,
            scroll_row: doc.scroll_row,
            scroll_col: doc.scroll_col,
            cursors: doc.cursors.clone(),
            primary: doc.primary,
            md_preview: false,
            md_scroll: 0,
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
    hover_divider: Option<usize>,
    hover_scrollbar: Option<PaneId>,
    hover_hscrollbar: Option<PaneId>,
    hover_shape: PointerShape,
    follow_cursor: bool,
    follow_cursor_h: bool,
    last_click: Option<(Instant, usize, usize, u8)>,
    pending_pane_pickers: VecDeque<(PaneId, PathBuf, usize, usize)>,
    term_panel: Option<IntegratedTerminal>,
    term_open: bool,
    term_focused: bool,
    term_height: usize,
    hover_term_divider: bool,
    hover_term_close: bool,
    completion: Option<CompletionPopup>,
    completion_dismissed_byte: Option<usize>,
    judge: JudgeRunner,
    diagnostics: HashMap<PathBuf, Vec<Diagnostic>>,
    diag_idx: usize,
    judge_doc_id: Option<usize>,
    cp_timer: CpTimer,
    cursor_trails: Vec<CursorTrail>,
    last_cursor_screen: Option<(usize, usize)>,
    arm_cursor_trail: bool,
    last_edit_sig: u64,
    pending_autosave_at: Option<Instant>,
    last_md_sync: (usize, usize, usize), // (doc_id, primary_row, doc_rev)
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
            hover_divider: None,
            hover_scrollbar: None,
            hover_hscrollbar: None,
            hover_shape: PointerShape::Default,
            follow_cursor: true,
            follow_cursor_h: true,
            last_click: None,
            pending_pane_pickers: VecDeque::new(),
            term_panel: None,
            term_open: false,
            term_focused: false,
            term_height: 0,
            hover_term_divider: false,
            hover_term_close: false,
            completion: None,
            completion_dismissed_byte: None,
            judge: JudgeRunner::new(),
            diagnostics: HashMap::new(),
            diag_idx: 0,
            judge_doc_id: None,
            cp_timer: CpTimer::default(),
            cursor_trails: Vec::new(),
            last_cursor_screen: None,
            arm_cursor_trail: false,
            last_edit_sig: 0,
            pending_autosave_at: None,
            last_md_sync: (usize::MAX, 0, 0),
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

    fn active_is_md_preview(&self) -> bool {
        self.pane(self.active_pane).active_view().md_preview
    }

    fn pane_md_geometry(&self, pid: PaneId, cols: usize, rows: usize) -> (usize, usize) {
        let area = self.pane_area(cols, rows);
        let computed = self.layout_tree.compute(area);
        let rect = computed
            .rect_of(pid)
            .unwrap_or(Rect { x: 0, y: 0, w: cols, h: rows.saturating_sub(1) });
        let sb_w = if self.cfg.scrollbar && rect.w >= 16 { 1 } else { 0 };
        let text_w = rect.w.saturating_sub(sb_w).max(8);
        let content_h = rect.h.saturating_sub(1).max(1);
        (text_w, content_h)
    }

    fn toggle_markdown_preview(&mut self, cols: usize, rows: usize) {
        self.clear_completion();
        self.cursor_trails.clear();
        self.sync_active_to_tab();
        let pid = self.active_pane;
        let currently_on = self.pane(pid).active_view().md_preview;

        if !currently_on {
            let (text_w, _) = self.pane_md_geometry(pid, cols, rows);
            let (primary_row, _) = self.doc().primary_line_col();
            let rendered = markdown::render_markdown(&self.doc().rope, text_w, self.palette);
            let vrow = rendered.visual_row_for_source(primary_row);
            let tv = self.pane_mut(pid).active_view_mut();
            tv.md_preview = true;
            tv.md_scroll = vrow.saturating_sub(2);
            self.set_status("◈ Modo Visualizacao Markdown (↑↓/j/k rolar · [/] secao · e/Alt+P editar · Ctrl+K m split)");
        } else {
            let cur_md_scroll = self.pane(pid).active_view().md_scroll;
            let target_src = self
                .layout
                .panes
                .iter()
                .find(|pl| pl.pane_id == pid && pl.md_preview)
                .and_then(|pl| pl.md_row_sources.first().copied())
                .unwrap_or_else(|| {
                    let (text_w, _) = self.pane_md_geometry(pid, cols, rows);
                    let rendered = markdown::render_markdown(&self.doc().rope, text_w, self.palette);
                    rendered
                        .lines
                        .get(cur_md_scroll)
                        .map(|l| l.source_line)
                        .unwrap_or(0)
                });
            self.pane_mut(pid).active_view_mut().md_preview = false;
            self.doc_mut().goto_line(target_src + 1);
            self.follow_cursor = true;
            self.set_status("Modo Edicao");
        }
    }

    fn open_markdown_preview_side(&mut self, cols: usize, rows: usize) {
        self.clear_completion();
        self.cursor_trails.clear();
        self.sync_active_to_tab();
        let cur_doc_id = self.active_doc_id();

        // Toggle OFF if another pane is already showing a live preview of this document
        if let Some(existing_pid) = self.panes.iter().find_map(|p| {
            if p.id != self.active_pane
                && p.active_view().doc_id == cur_doc_id
                && p.active_view().md_preview
            {
                Some(p.id)
            } else {
                None
            }
        }) {
            if self.panes.len() > 1 {
                self.panes.retain(|p| p.id != existing_pid);
                if let Some(next_pid) = self.layout_tree.remove_pane(existing_pid) {
                    if self.active_pane == existing_pid {
                        self.active_pane = next_pid;
                    }
                    self.load_active_from_tab();
                }
                self.set_status("Preview Markdown ao lado fechado");
                return;
            }
        }

        // Ensure source pane is in Edit mode before splitting live preview to the right
        let src_pid = self.active_pane;
        self.pane_mut(src_pid).active_view_mut().md_preview = false;
        let before_count = self.panes.len();
        self.split_active(SplitAxis::Horizontal, false, cols, rows);
        if self.panes.len() == before_count {
            return; // terminal too narrow to split
        }
        let preview_pid = self.active_pane;
        let (text_w, _) = self.pane_md_geometry(preview_pid, cols, rows);
        let (primary_row, _) = self.doc().primary_line_col();
        let rendered = markdown::render_markdown(&self.doc().rope, text_w, self.palette);
        let vrow = rendered.visual_row_for_source(primary_row);
        {
            let tv = self.pane_mut(preview_pid).active_view_mut();
            tv.md_preview = true;
            tv.md_scroll = vrow.saturating_sub(3);
        }
        // Keep focus in the source pane so user edits live while watching preview!
        self.active_pane = src_pid;
        self.load_active_from_tab();
        self.set_status("◈ Preview Markdown ao Lado (edite aqui, visualize em tempo real a direita · Ctrl+K m fecha)");
    }

    fn scroll_md_pane(&mut self, pid: PaneId, delta: isize, cols: usize, rows: usize) {
        let (tot, view_h) = if let Some(pl) = self.layout.panes.iter().find(|p| p.pane_id == pid && p.md_preview) {
            (pl.md_total_lines, pl.content_height.max(1))
        } else {
            let (text_w, ch) = self.pane_md_geometry(pid, cols, rows);
            let did = self.pane(pid).active_view().doc_id;
            let tot = self
                .docs
                .get(did)
                .and_then(|d| d.as_ref())
                .map(|d| markdown::render_markdown(&d.rope, text_w, self.palette).total_lines())
                .unwrap_or(1);
            (tot, ch)
        };
        let max_scroll = tot.saturating_sub(view_h);
        let tv = self.pane_mut(pid).active_view_mut();
        tv.md_scroll = if delta < 0 {
            tv.md_scroll.saturating_sub(delta.unsigned_abs())
        } else {
            tv.md_scroll.saturating_add(delta as usize).min(max_scroll)
        };
    }

    fn jump_md_heading(&mut self, forward: bool, cols: usize, rows: usize) {
        let pid = self.active_pane;
        let (text_w, _) = self.pane_md_geometry(pid, cols, rows);
        let cur = self.pane(pid).active_view().md_scroll;
        let rendered = markdown::render_markdown(&self.doc().rope, text_w, self.palette);
        if let Some((target, title)) = rendered.jump_heading(cur, forward) {
            self.pane_mut(pid).active_view_mut().md_scroll = target;
            self.set_status(format!("◈ {}", title));
        } else {
            self.set_status("Nenhum titulo (#) encontrado neste documento");
        }
    }

    fn arm_trail(&mut self) {
        if self.cfg.cursor_trail {
            self.arm_cursor_trail = true;
        }
    }

    fn focus_pane_and_tab(&mut self, pane_id: PaneId, tab_idx: usize) {
        if !self.panes.iter().any(|p| p.id == pane_id) {
            return;
        }
        self.arm_trail();
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

    fn bottom_reserved_rows(&self) -> usize {
        let has_prompt = matches!(
            self.mode,
            Mode::Find
                | Mode::Replace { .. }
                | Mode::GotoLine(_)
                | Mode::SaveAs(_)
                | Mode::ConfirmQuit
                | Mode::ConfirmCloseTab
        );
        if has_prompt { 2 } else { 1 }
    }

    /// Returns `Some((header_row, body_height))` when the integrated terminal drawer is visible.
    fn term_layout(&self, rows: usize) -> Option<(usize, usize)> {
        if !self.term_open || self.term_panel.is_none() {
            return None;
        }
        let usable_h = rows.saturating_sub(self.bottom_reserved_rows());
        let min_editor_h = (MIN_PANE_H + 1).max(4);
        if usable_h < min_editor_h + 4 {
            return None;
        }
        let max_body = usable_h.saturating_sub(min_editor_h + 1).max(3);
        let desired = if self.term_height == 0 {
            (usable_h / 3).clamp(6, 16)
        } else {
            self.term_height
        };
        let body_h = desired.clamp(3, max_body);
        let header_row = usable_h.saturating_sub(body_h + 1);
        Some((header_row, body_h))
    }

    fn pane_area(&self, cols: usize, rows: usize) -> Rect {
        let h = if let Some((header_row, _)) = self.term_layout(rows) {
            header_row.max(2)
        } else {
            rows.saturating_sub(self.bottom_reserved_rows()).max(2)
        };
        Rect { x: 0, y: 0, w: cols, h }
    }

    fn term_pty_fd(&self) -> Option<std::os::unix::io::RawFd> {
        self.term_panel
            .as_ref()
            .filter(|t| t.alive)
            .map(|t| t.master_fd())
    }

    fn drain_terminal_output(&mut self) -> bool {
        let mut changed = false;
        let mut exited = false;
        if let Some(ref mut t) = self.term_panel {
            if t.drain_output() {
                changed = true;
            }
            if !t.alive {
                exited = true;
            }
        }
        if exited {
            self.term_panel = None;
            self.term_open = false;
            self.term_focused = false;
            self.hover_term_divider = false;
            self.hover_term_close = false;
            self.set_status("Terminal encerrado");
            changed = true;
        }
        changed
    }

    fn toggle_terminal(&mut self, cols: usize, rows: usize) {
        if self.term_open && self.term_panel.is_some() {
            if self.term_focused {
                // Hide drawer (keep shell alive in background!) and focus editor
                self.term_open = false;
                self.term_focused = false;
                self.hover_term_divider = false;
                self.hover_term_close = false;
                self.set_status("Terminal oculto em segundo plano (Ctrl+' reabre)");
            } else {
                // Already visible: focus it immediately
                self.term_focused = true;
                self.mode = Mode::Edit;
                self.set_status("Foco no Terminal (Ctrl+' ocultar · Ctrl+1..9 editor)");
            }
            return;
        }

        if self.term_panel.is_none() {
            let cwd = self.base_dir();
            let init_h = ((rows.saturating_sub(2)) / 3).clamp(6, 16);
            match IntegratedTerminal::spawn(&cwd, cols, init_h) {
                Ok(t) => {
                    self.term_panel = Some(t);
                }
                Err(e) => {
                    self.set_status(format!("Erro ao abrir terminal: {}", e));
                    return;
                }
            }
        }
        self.term_open = true;
        self.term_focused = true;
        self.mode = Mode::Edit;
        if let Some((_, body_h)) = self.term_layout(rows) {
            if let Some(ref mut t) = self.term_panel {
                t.resize(cols, body_h);
            }
        }
        self.set_status("Terminal integrado aberto (Ctrl+' ocultar · Ctrl+1..9 editor)");
    }

    fn restart_terminal(&mut self, cols: usize, rows: usize) {
        self.term_panel = None;
        self.term_open = false;
        self.term_focused = false;
        self.toggle_terminal(cols, rows);
    }

    fn kill_terminal(&mut self) {
        self.term_panel = None;
        self.term_open = false;
        self.term_focused = false;
        self.hover_term_divider = false;
        self.hover_term_close = false;
        self.hover_shape = PointerShape::Default;
        self.set_status("Terminal fechado");
    }

    // -------------------------------------------------------------------------
    // Competitive Programming Suite: Autocomplete, Snippets, Judge, Stress, Symbols, LeetCode
    // -------------------------------------------------------------------------

    fn clear_completion(&mut self) {
        self.completion = None;
    }

    fn completion_popup_visible(&self) -> bool {
        self.completion
            .as_ref()
            .is_some_and(|c| self.cfg.autocomplete_popup || c.forced)
    }

    fn refresh_completion(&mut self, forced: bool) {
        if !matches!(self.mode, Mode::Edit) || (self.term_open && self.term_focused) {
            self.completion = None;
            return;
        }
        if self.doc().cursors.len() != 1 || !self.doc().primary_cursor().is_empty() {
            self.completion = None;
            return;
        }
        if !forced && !self.cfg.autocomplete {
            self.completion = None;
            return;
        }
        let head = self.doc().primary_cursor().head;
        if !forced && self.completion_dismissed_byte == Some(head) {
            self.completion = None;
            return;
        }
        self.completion_dismissed_byte = None;
        self.completion = compute_completions(
            &self.doc().rope,
            head,
            self.doc().lang,
            self.cfg.autocomplete_min_len,
            self.cfg.exam_mode,
            forced,
        );
    }

    fn indent_multiline_for_cursor(&self, text: &str) -> String {
        if !text.contains('\n') {
            return text.to_string();
        }
        let (row, _) = self.doc().primary_line_col();
        let cur_line = self.doc().rope.line(row);
        let base_indent: String = cur_line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        if base_indent.is_empty() {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len() + 32);
        for (i, part) in text.split('\n').enumerate() {
            if i > 0 {
                out.push('\n');
                if !part.is_empty() {
                    out.push_str(&base_indent);
                }
            }
            out.push_str(part);
        }
        out
    }

    fn accept_completion(&mut self) -> bool {
        let Some(pop) = self.completion.take() else {
            return false;
        };
        let Some(item) = pop.active().cloned() else {
            return false;
        };
        let head = self.doc().primary_cursor().head;
        let replacement = if item.kind == CompletionKind::Snippet {
            self.indent_multiline_for_cursor(&item.insert)
        } else {
            item.insert.clone()
        };
        self.doc_mut()
            .replace_byte_range_atomic(pop.start_byte, head, &replacement);
        if item.kind == CompletionKind::Snippet {
            self.set_status(format!("✓ Snippet expandido: {}", item.detail));
        }
        true
    }

    fn try_expand_exact_snippet_on_tab(&mut self) -> bool {
        if self.cfg.exam_mode || self.doc().cursors.len() != 1 || !self.doc().primary_cursor().is_empty() {
            return false;
        }
        let head = self.doc().primary_cursor().head;
        let (row, col) = self.doc().rope.byte_to_line_col(head);
        let line = self.doc().rope.line(row);
        let line_start = self.doc().rope.line_start_byte(row);
        let col_byte = line.char_indices().nth(col).map(|(b, _)| b).unwrap_or(line.len());
        let before = &line[..col_byte];
        let bytes = before.as_bytes();
        let mut s = bytes.len();
        while s > 0 && (bytes[s - 1].is_ascii_alphanumeric() || bytes[s - 1] == b'_') {
            s -= 1;
        }
        let trigger = &before[s..];
        if trigger.is_empty() {
            return false;
        }
        let Some(snip) = find_snippet(trigger, self.doc().lang) else {
            return false;
        };
        let body = self.indent_multiline_for_cursor(&snip.body);
        self.doc_mut()
            .replace_byte_range_atomic(line_start + s, head, &body);
        self.set_status(format!("✓ Template '{}' inserido", snip.trigger));
        true
    }

    fn insert_snippet_at_cursor(&mut self, body: &str) {
        let indented = self.indent_multiline_for_cursor(body);
        let head = self.doc().primary_cursor().head;
        self.doc_mut().replace_byte_range_atomic(head, head, &indented);
        self.set_status("✓ Template CP inserido");
    }

    fn active_doc_key(&self) -> Option<PathBuf> {
        self.doc()
            .path
            .as_ref()
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
    }

    fn resolve_judge_target_path(&self) -> Option<PathBuf> {
        let abs_of = |p: &Path| -> PathBuf {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(p))
                    .unwrap_or_else(|_| p.to_path_buf())
            }
        };

        // 1. Active document if it is already a runnable source file (.cpp/.py/.rs/etc.)
        if let Some(p) = self.doc().path.as_ref() {
            if is_runnable_source(p) {
                return Some(abs_of(p));
            }
        }

        // 2. If user hit F5/F6 while focused on in.txt / expected.txt / [Judge], pick the open source file!
        let cur_dir = self
            .doc()
            .path
            .as_deref()
            .map(resolve_src_dir)
            .unwrap_or_else(|| self.base_dir());
        let mut fallback: Option<PathBuf> = None;
        for pid in self.layout_tree.pane_order() {
            if let Some(pane) = self.panes.iter().find(|p| p.id == pid) {
                // Check active tab of each pane first, then remaining tabs
                let mut indices: Vec<usize> = vec![pane.active_tab];
                for i in 0..pane.tabs.len() {
                    if i != pane.active_tab {
                        indices.push(i);
                    }
                }
                for ti in indices {
                    if let Some(did) = pane.tabs.get(ti).map(|t| t.doc_id) {
                        if let Some(p) = self.docs.get(did).and_then(|d| d.as_ref()).and_then(|d| d.path.as_ref()) {
                            if is_runnable_source(p) {
                                let ap = abs_of(p);
                                if resolve_src_dir(&ap) == cur_dir {
                                    return Some(ap);
                                }
                                if fallback.is_none() {
                                    fallback = Some(ap);
                                }
                            }
                        }
                    }
                }
            }
        }
        if fallback.is_some() {
            return fallback;
        }

        // 3. Fallback: conventional source file sitting in the same folder as in.txt / expected.txt
        for name in ["sol.cpp", "main.cpp", "a.cpp", "sol.py", "main.py", "main.rs", "main.c"] {
            let cand = cur_dir.join(name);
            if cand.is_file() {
                return Some(cand);
            }
        }
        None
    }

    fn run_judge(&mut self, debug_mode: bool) {
        self.clear_completion();
        // Flush ALL open dirty buffers (sol.cpp, in.txt, expected.txt, etc.) so Judge reads fresh edits!
        self.flush_auto_save();
        if self.doc().path.is_some() && self.doc().is_dirty() && !self.try_save_active() {
            return;
        }
        let Some(src_path) = self.resolve_judge_target_path() else {
            self.set_status("Abra/salve um arquivo de codigo (.cpp/.py/.rs) antes de julgar (F5)");
            return;
        };
        let key = src_path.canonicalize().unwrap_or_else(|_| src_path.clone());
        self.diagnostics.remove(&key);
        self.diag_idx = 0;
        self.judge
            .start_judge(src_path, debug_mode, self.cfg.judge_timeout_ms);
    }

    fn run_stress_test(&mut self) {
        self.clear_completion();
        self.flush_auto_save();
        if self.doc().path.is_some() && self.doc().is_dirty() && !self.try_save_active() {
            return;
        }
        let Some(src_path) = self.resolve_judge_target_path() else {
            self.set_status("Abra e salve sol.cpp primeiro para rodar Stress Test");
            return;
        };
        self.judge
            .start_stress_test(src_path, self.cfg.judge_timeout_ms, 500);
    }

    fn scaffold_stress_in_current_dir(&mut self) {
        let dir = self.base_dir();
        match scaffold_stress_files(&dir) {
            Ok((gen_p, brute_p)) => {
                self.set_status(format!(
                    "✓ Criados {} e {} (use Ctrl+P p/ abrir e F1 -> Stress Test)",
                    gen_p.file_name().and_then(|s| s.to_str()).unwrap_or("gen.cpp"),
                    brute_p.file_name().and_then(|s| s.to_str()).unwrap_or("brute.cpp")
                ));
            }
            Err(e) => self.set_status(format!("Erro ao gerar esqueleto stress: {}", e)),
        }
    }

    fn show_judge_report_pane(&mut self, report: &str, cols: usize, rows: usize) {
        let orig_pane = self.active_pane;
        self.sync_active_to_tab();

        // Reuse or allocate virtual `[Judge]` document
        let j_did = if let Some(did) = self.judge_doc_id.filter(|&id| self.docs.get(id).and_then(|d| d.as_ref()).is_some()) {
            if let Some(doc) = self.docs[did].as_mut() {
                doc.set_virtual_text("[Judge]", report);
            }
            did
        } else {
            let mut d = Document::new_empty();
            d.set_virtual_text("[Judge]", report);
            let did = self.alloc_doc(d);
            self.judge_doc_id = Some(did);
            did
        };

        // Pick a non-focused pane (last pane in layout) or create a right split if single-pane
        let order = self.layout_tree.pane_order();
        let target_pane = if let Some(&other) = order.iter().rev().find(|&&pid| pid != orig_pane) {
            other
        } else if cols >= MIN_PANE_W * 2 + 1 {
            let new_pid = self.next_pane_id;
            self.next_pane_id += 1;
            let split_id = self.next_split_id;
            self.next_split_id += 1;
            let tv = TabView::from_doc(j_did, self.docs[j_did].as_ref().unwrap());
            self.panes.push(Pane {
                id: new_pid,
                tabs: vec![tv],
                active_tab: 0,
            });
            self.layout_tree
                .split_pane(orig_pane, new_pid, split_id, SplitAxis::Horizontal, false);
            self.layout_tree.set_split_ratio(split_id, 0.62);
            let _ = rows;
            return;
        } else {
            return;
        };

        let tv = TabView::from_doc(j_did, self.docs[j_did].as_ref().unwrap());
        let p = self.pane_mut(target_pane);
        if let Some(tidx) = p.tabs.iter().position(|t| t.doc_id == j_did) {
            p.tabs[tidx] = tv;
            p.active_tab = tidx;
        } else {
            p.tabs.push(tv);
            p.active_tab = p.tabs.len() - 1;
        }
        // Keep focus strictly on original coding pane!
        self.active_pane = orig_pane;
        self.load_active_from_tab();
    }

    fn poll_judge_updates(&mut self, cols: usize, rows: usize) -> bool {
        if !self.judge.take_dirty() {
            return false;
        }
        if let Some(res) = self.judge.take_result() {
            let key = res
                .src_path
                .canonicalize()
                .unwrap_or_else(|_| res.src_path.clone());
            if res.diagnostics.is_empty() {
                self.diagnostics.remove(&key);
            } else {
                self.diagnostics.insert(key, res.diagnostics.clone());
            }
            self.diag_idx = 0;

            // If Stress Test saved a counterexample to `in.txt`, live-reload any open `in.txt` buffer!
            if let Some(ref in_p) = res.updated_in_txt {
                let canon_in = in_p.canonicalize().unwrap_or_else(|_| in_p.clone());
                for d in self.docs.iter_mut().flatten() {
                    if let Some(ref dp) = d.path {
                        if dp.canonicalize().unwrap_or_else(|_| dp.clone()) == canon_in {
                            let _ = d.reload_from_disk();
                        }
                    }
                }
            }

            self.show_judge_report_pane(&res.report, cols, rows);
            self.set_status(res.summary);
        }
        true
    }

    fn jump_next_diagnostic(&mut self) {
        let Some(key) = self.active_doc_key() else {
            self.set_status("Nenhum diagnostico neste arquivo");
            return;
        };
        let Some(list) = self.diagnostics.get(&key).filter(|v| !v.is_empty()).cloned() else {
            self.set_status("✓ Nenhum erro ou warning de compilacao");
            return;
        };
        let idx = self.diag_idx % list.len();
        self.diag_idx = (idx + 1) % list.len();
        let d = &list[idx];
        self.arm_trail();
        self.doc_mut().goto_line(d.line + 1);
        let tag = match d.severity {
            DiagSeverity::Error => "● Erro",
            DiagSeverity::Warning => "▲ Warn",
        };
        self.set_status(format!(
            "{} {}/{} (Ln {}): {}",
            tag,
            idx + 1,
            list.len(),
            d.line + 1,
            d.message
        ));
    }

    fn setup_cp_layout(&mut self, cols: usize, rows: usize) {
        self.clear_completion();
        self.sync_active_to_tab();
        let dir = self.base_dir();

        // Ensure active tab has a source file (if blank untitled, open/create sol.cpp)
        if self.doc().path.is_none() && self.doc().rope.is_empty() {
            let sol_p = dir.join("sol.cpp");
            self.open_or_focus_file(&sol_p, None);
        }
        let main_tv = self.pane(self.active_pane).active_view().clone();

        // Open `in.txt` and `expected.txt` (or existing `out.txt`)
        let in_path = dir.join("in.txt");
        let exp_path = if dir.join("expected.txt").exists() || !dir.join("out.txt").exists() {
            dir.join("expected.txt")
        } else {
            dir.join("out.txt")
        };

        let open_or_alloc = |docs: &mut Vec<Option<Document>>, cfg_ac: bool, path: &Path| -> usize {
            let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            if let Some((id, _)) = docs.iter().enumerate().find(|(_, d)| {
                d.as_ref()
                    .and_then(|x| x.path.as_ref())
                    .is_some_and(|p| p.canonicalize().unwrap_or_else(|_| p.clone()) == canon)
            }) {
                return id;
            }
            let mut d = Document::open(path, None).unwrap_or_else(|_| Document::new_empty());
            d.auto_close = cfg_ac;
            if let Some(slot) = docs.iter().position(|x| x.is_none()) {
                docs[slot] = Some(d);
                slot
            } else {
                let id = docs.len();
                docs.push(Some(d));
                id
            }
        };

        let in_did = open_or_alloc(&mut self.docs, self.cfg.auto_close, &in_path);
        let exp_did = open_or_alloc(&mut self.docs, self.cfg.auto_close, &exp_path);
        let in_tv = TabView::from_doc(in_did, self.docs[in_did].as_ref().unwrap());
        let exp_tv = TabView::from_doc(exp_did, self.docs[exp_did].as_ref().unwrap());

        let p_code = self.next_pane_id;
        let p_in = p_code + 1;
        let p_exp = p_code + 2;
        self.next_pane_id += 3;
        let s_main = self.next_split_id;
        let s_right = s_main + 1;
        self.next_split_id += 2;

        self.panes = vec![
            Pane { id: p_code, tabs: vec![main_tv], active_tab: 0 },
            Pane { id: p_in, tabs: vec![in_tv], active_tab: 0 },
            Pane { id: p_exp, tabs: vec![exp_tv], active_tab: 0 },
        ];
        let mut tree = LayoutNode::leaf(p_code);
        tree.split_pane(p_code, p_in, s_main, SplitAxis::Horizontal, false);
        tree.set_split_ratio(s_main, 0.62);
        tree.split_pane(p_in, p_exp, s_right, SplitAxis::Vertical, false);
        tree.set_split_ratio(s_right, 0.50);
        self.layout_tree = tree;
        self.active_pane = p_code;
        self.load_active_from_tab();
        let _ = (cols, rows);
        self.set_status("✓ Layout CP montado: ① codigo | ② in.txt / ③ expected.txt (F5 julgar · F6 debug)");
    }

    fn open_symbol_picker(&mut self) {
        self.clear_completion();
        self.pending_pane_pickers.clear();
        let syms = extract_symbols(&self.doc().rope, self.doc().lang);
        if syms.is_empty() {
            self.set_status("Nenhum simbolo (funcao/struct/class) encontrado neste arquivo");
            return;
        }
        self.mode = Mode::Picker(Picker::symbols_picker(&syms));
    }

    fn open_snippets_picker(&mut self) {
        self.clear_completion();
        if self.cfg.exam_mode {
            self.set_status("Modo Prova [ON]: snippets bloqueados (desative em F1 para usar)");
            return;
        }
        self.pending_pane_pickers.clear();
        let snips = all_snippets(self.doc().lang);
        self.mode = Mode::Picker(Picker::snippets_picker(&snips));
    }

    fn copy_leetcode_solution(&mut self) {
        match extract_leetcode_solution(&self.doc().rope, self.doc().lang) {
            Some(sol) => {
                let lines = sol.lines().count();
                self.clipboard = sol.clone();
                copy_to_system_clipboard(&sol);
                self.set_status(format!(
                    "✓ Solution limpa copiada p/ clipboard ({} linhas · pronto p/ LeetCode)",
                    lines
                ));
            }
            None => {
                self.set_status("Nenhuma 'class Solution' ou 'struct Solution' encontrada neste arquivo");
            }
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

    fn focus_pane_by_number(&mut self, one_based: usize, cols: usize, rows: usize) {
        if one_based == 0 {
            return;
        }
        let order = self.layout_tree.pane_order();
        if let Some(&pid) = order.get(one_based - 1) {
            self.focus_pane(pid);
            self.set_status(format!("Painel {}/{}", one_based, order.len()));
            return;
        }

        // VS Code behavior: Ctrl+2/3/4... creates the missing split pane(s) automatically
        // and immediately opens the fuzzy file picker in the current file's directory!
        let max_panes = ((cols + 1) / (MIN_PANE_W + 1)).max(1);
        let target_total = one_based.min(max_panes);
        if target_total <= order.len() {
            self.set_status(format!(
                "Terminal estreito demais para abrir Painel {} (max {})",
                one_based, max_panes
            ));
            return;
        }

        let origin_dir = self.base_dir();
        self.sync_active_to_tab();

        let to_create = target_total - order.len();
        let mut created_pids = Vec::with_capacity(to_create);
        for _ in 0..to_create {
            let last_pid = self
                .layout_tree
                .pane_order()
                .last()
                .copied()
                .unwrap_or(self.active_pane);
            let blank_doc = Document::new_empty();
            let mut tv = TabView::from_doc(0, &blank_doc);
            let did = self.alloc_doc(blank_doc);
            tv.doc_id = did;

            let new_pid = self.next_pane_id;
            self.next_pane_id += 1;
            let split_id = self.next_split_id;
            self.next_split_id += 1;

            self.panes.push(Pane {
                id: new_pid,
                tabs: vec![tv],
                active_tab: 0,
            });
            self.layout_tree
                .split_pane(last_pid, new_pid, split_id, SplitAxis::Horizontal, false);
            created_pids.push(new_pid);
        }

        self.layout_tree.equalize_all();
        self.pending_pane_pickers.clear();
        let total_steps = created_pids.len();
        for (i, pid) in created_pids.into_iter().enumerate() {
            self.pending_pane_pickers
                .push_back((pid, origin_dir.clone(), i + 1, total_steps));
        }
        let _ = rows;
        self.open_next_pending_pane_picker();
    }

    fn open_next_pending_pane_picker(&mut self) -> bool {
        while let Some((pid, origin_dir, step, total_steps)) = self.pending_pane_pickers.pop_front()
        {
            if !self.panes.iter().any(|p| p.id == pid) {
                continue;
            }
            self.focus_pane(pid);
            let num = self.group_number_of(pid);
            let prefix = if total_steps > 1 {
                format!("Painel {} ({}/{})", num, step, total_steps)
            } else {
                format!("Painel {}", num)
            };
            self.mode = Mode::Picker(Picker::quick_open_titled(&origin_dir, Some(&prefix)));
            if let Some(&(next_pid, _, _, _)) = self.pending_pane_pickers.front() {
                let next_num = self.group_number_of(next_pid);
                self.set_status(format!(
                    "Painel {} ({}/{}) · Enter abrir · Esc pular p/ Painel {}",
                    num, step, total_steps, next_num
                ));
            } else {
                self.set_status(format!(
                    "Painel {} aberto · escolha/digite arquivo ou Esc p/ vazio",
                    num
                ));
            }
            return true;
        }
        false
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

    fn handle_event(&mut self, ev: Event, cols: usize, rows: usize) -> bool {
        match ev {
            Event::Paste(text) => {
                self.follow_cursor = true;
                self.follow_cursor_h = true;
                self.handle_paste(&text);
                true
            }
            Event::Mouse(m) => self.handle_mouse(m, cols, rows),
            Event::Key(k) => {
                let is_pure_vertical_nav = matches!(self.mode, Mode::Edit)
                    && !k.alt
                    && matches!(
                        k.code,
                        KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown
                    );
                self.follow_cursor = true;
                let had_h_pan_lock = !self.follow_cursor_h;
                if !is_pure_vertical_nav {
                    self.follow_cursor_h = true;
                }
                self.handle_key(k, cols, rows);
                if is_pure_vertical_nav {
                    if had_h_pan_lock {
                        // User explicitly panned horizontally via touchpad/scrollbar: keep reading column locked!
                        self.follow_cursor_h = false;
                    } else if !self.cfg.wrap && self.doc().scroll_col > 0 {
                        // Moving vertically across a shorter/blank line while desired_col is still
                        // to the right of scroll_col should NOT yank the viewport back to col 0!
                        let d = self.doc();
                        let desired = d
                            .cursors
                            .get(d.primary.min(d.cursors.len().saturating_sub(1)))
                            .map(|s| s.desired_col)
                            .unwrap_or(0);
                        let (_, pcol) = d.primary_line_col();
                        if pcol < desired && desired >= d.scroll_col {
                            self.follow_cursor_h = false;
                        }
                    }
                }
                true
            }
        }
    }

    fn handle_paste(&mut self, text: &str) {
        if matches!(self.mode, Mode::Edit) && self.term_open && self.term_focused {
            if let Some(ref mut t) = self.term_panel {
                t.send_paste(text);
                return;
            }
        }
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

    fn update_hover(&mut self, col: usize, row: usize, cols: usize, rows: usize) -> bool {
        let prev_div = self.hover_divider;
        let prev_sb = self.hover_scrollbar;
        let prev_hsb = self.hover_hscrollbar;
        let prev_tdiv = self.hover_term_divider;
        let prev_tclose = self.hover_term_close;

        if matches!(self.mode, Mode::Picker(_)) {
            self.hover_divider = None;
            self.hover_scrollbar = None;
            self.hover_hscrollbar = None;
            self.hover_term_divider = false;
            self.hover_term_close = false;
            self.hover_shape = PointerShape::Default;
            return prev_div != self.hover_divider
                || prev_sb != self.hover_scrollbar
                || prev_hsb != self.hover_hscrollbar
                || prev_tdiv != self.hover_term_divider
                || prev_tclose != self.hover_term_close;
        }

        if let Some((header_row, _)) = self.term_layout(rows) {
            if row == header_row {
                self.hover_divider = None;
                self.hover_scrollbar = None;
                self.hover_hscrollbar = None;
                if cols >= 3 && col >= cols - 3 {
                    self.hover_term_close = true;
                    self.hover_term_divider = false;
                    self.hover_shape = PointerShape::Grab;
                } else {
                    self.hover_term_divider = true;
                    self.hover_term_close = false;
                    self.hover_shape = PointerShape::ResizeV;
                }
                return prev_div != self.hover_divider
                    || prev_sb != self.hover_scrollbar
                    || prev_hsb != self.hover_hscrollbar
                    || prev_tdiv != self.hover_term_divider
                    || prev_tclose != self.hover_term_close;
            }
        }
        self.hover_term_divider = false;
        self.hover_term_close = false;

        if let Some(d) = self.layout.dividers.iter().copied().find(|d| d.hits(col, row)) {
            self.hover_divider = Some(d.split_id);
            self.hover_scrollbar = None;
            self.hover_hscrollbar = None;
            self.hover_shape = match d.axis {
                SplitAxis::Horizontal => PointerShape::ResizeH,
                SplitAxis::Vertical => PointerShape::ResizeV,
            };
        } else if let Some(pl) = self.layout.panes.iter().find(|pl| {
            pl.scrollbar_col == Some(col)
                && row >= pl.content_start_row
                && row < pl.content_start_row + pl.content_height
        }) {
            self.hover_divider = None;
            self.hover_scrollbar = Some(pl.pane_id);
            self.hover_hscrollbar = None;
            self.hover_shape = PointerShape::Grab;
        } else if let Some(pl) = self.layout.panes.iter().find(|pl| {
            pl.hscrollbar.is_some_and(|(hrow, _, _, _)| row == hrow && col >= pl.rect.x && col < pl.rect.x + pl.rect.w)
        }) {
            self.hover_divider = None;
            self.hover_scrollbar = None;
            self.hover_hscrollbar = Some(pl.pane_id);
            self.hover_shape = PointerShape::Grab;
        } else if self.layout.panes.iter().any(|pl| {
            row == pl.tab_bar_row
                && pl
                    .tab_hitboxes
                    .iter()
                    .any(|&(sc, ec, _)| col >= sc && col < ec)
        }) {
            self.hover_divider = None;
            self.hover_scrollbar = None;
            self.hover_hscrollbar = None;
            self.hover_shape = PointerShape::Grab;
        } else {
            self.hover_divider = None;
            self.hover_scrollbar = None;
            self.hover_hscrollbar = None;
            self.hover_shape = PointerShape::Default;
        }

        prev_div != self.hover_divider
            || prev_sb != self.hover_scrollbar
            || prev_hsb != self.hover_hscrollbar
            || prev_tdiv != self.hover_term_divider
            || prev_tclose != self.hover_term_close
    }

    fn scroll_pane_from_hscrollbar(
        &mut self,
        pane_id: PaneId,
        mouse_col: usize,
        start_col: usize,
        track_w: usize,
        max_scroll_col: usize,
    ) {
        if track_w == 0 {
            return;
        }
        let tot_w = (max_scroll_col + track_w).max(1);
        let thumb_w = ((track_w * track_w) / tot_w).clamp(3.min(track_w), track_w.max(1));
        let travel = track_w.saturating_sub(thumb_w);
        let rel = mouse_col.saturating_sub(start_col).min(track_w.saturating_sub(1));
        let centered = rel.saturating_sub(thumb_w / 2).min(travel);
        let target = if travel == 0 || max_scroll_col == 0 {
            0
        } else {
            (centered * max_scroll_col) / travel
        };
        if pane_id == self.active_pane {
            self.doc_mut().scroll_col = target;
            self.follow_cursor_h = false;
        } else if self.panes.iter().any(|p| p.id == pane_id) {
            self.pane_mut(pane_id).active_view_mut().scroll_col = target;
        }
    }

    fn scroll_pane_horizontal(&mut self, pane_id: PaneId, delta: isize) {
        if self.cfg.wrap {
            return;
        }
        let max_sc = self
            .layout
            .panes
            .iter()
            .find(|pl| pl.pane_id == pane_id)
            .and_then(|pl| pl.hscrollbar.map(|(_, _, _, m)| m))
            .unwrap_or_else(|| {
                let Some(rect) = self.last_computed.rect_of(pane_id) else {
                    return 0;
                };
                let (did, sr, sc) = if pane_id == self.active_pane {
                    (self.active_doc_id(), self.doc().scroll_row, self.doc().scroll_col)
                } else {
                    let tv = self.pane(pane_id).active_view();
                    (tv.doc_id, tv.scroll_row, tv.scroll_col)
                };
                self.docs
                    .get(did)
                    .and_then(|d| d.as_ref())
                    .and_then(|d| pane_content_height(d, rect.w, rect.h, sr, sc, &self.cfg).1)
                    .unwrap_or(0)
            });

        if pane_id == self.active_pane {
            let cur = self.doc().scroll_col;
            let next = if delta < 0 {
                cur.saturating_sub((-delta) as usize)
            } else {
                cur.saturating_add(delta as usize).min(max_sc)
            };
            self.doc_mut().scroll_col = next;
            self.follow_cursor_h = false;
        } else if self.panes.iter().any(|p| p.id == pane_id) {
            let tv = self.pane_mut(pane_id).active_view_mut();
            let cur = tv.scroll_col;
            tv.scroll_col = if delta < 0 {
                cur.saturating_sub((-delta) as usize)
            } else {
                cur.saturating_add(delta as usize).min(max_sc)
            };
        }
    }

    fn scroll_pane_from_scrollbar(
        &mut self,
        pane_id: PaneId,
        mouse_row: usize,
        start_row: usize,
        height: usize,
    ) {
        if height == 0 {
            return;
        }
        let rel = mouse_row.saturating_sub(start_row).min(height.saturating_sub(1));
        if self.panes.iter().any(|p| p.id == pane_id && p.active_view().md_preview) {
            let total = self
                .layout
                .panes
                .iter()
                .find(|pl| pl.pane_id == pane_id)
                .map(|pl| pl.md_total_lines)
                .unwrap_or(1);
            let max_scroll = total.saturating_sub(height);
            let target = if height <= 1 || max_scroll == 0 {
                0
            } else {
                (rel * max_scroll) / (height - 1)
            };
            self.pane_mut(pane_id).active_view_mut().md_scroll = target;
            return;
        }
        if pane_id == self.active_pane {
            let total = self.doc().rope.line_count();
            let max_scroll = total.saturating_sub(height);
            let target = if height <= 1 || max_scroll == 0 {
                0
            } else {
                (rel * max_scroll) / (height - 1)
            };
            self.doc_mut().scroll_row = target;
            self.follow_cursor = false;
        } else if self.panes.iter().any(|p| p.id == pane_id) {
            let did = self.pane(pane_id).active_view().doc_id;
            let total = self
                .docs
                .get(did)
                .and_then(|d| d.as_ref())
                .map(|d| d.rope.line_count())
                .unwrap_or(0);
            let max_scroll = total.saturating_sub(height);
            let target = if height <= 1 || max_scroll == 0 {
                0
            } else {
                (rel * max_scroll) / (height - 1)
            };
            self.pane_mut(pane_id).active_view_mut().scroll_row = target;
        }
    }

    fn handle_mouse(&mut self, m: input::MouseEvent, cols: usize, rows: usize) -> bool {
        if matches!(self.mode, Mode::Picker(_)) {
            if let Mode::Picker(ref mut pk) = self.mode {
                match m.kind {
                    MouseKind::WheelUp => {
                        pk.move_sel(-1);
                        return true;
                    }
                    MouseKind::WheelDown => {
                        pk.move_sel(1);
                        return true;
                    }
                    MouseKind::Move => {
                        self.hover_shape = PointerShape::Default;
                        return false;
                    }
                    _ => {}
                }
            }
            return false;
        }

        match m.kind {
            MouseKind::Move => {
                return self.update_hover(m.col, m.row, cols, rows);
            }
            MouseKind::WheelLeft | MouseKind::WheelRight => {
                let target_pid = self
                    .layout
                    .panes
                    .iter()
                    .find(|pl| pl.rect.contains(m.col, m.row))
                    .map(|pl| pl.pane_id)
                    .unwrap_or(self.active_pane);
                let delta = if m.kind == MouseKind::WheelLeft { -4 } else { 4 };
                self.scroll_pane_horizontal(target_pid, delta);
                true
            }
            MouseKind::WheelUp | MouseKind::WheelDown => {
                if let Some((header_row, body_h)) = self.term_layout(rows) {
                    if m.row > header_row && m.row < header_row + 1 + body_h {
                        if let Some(ref mut t) = self.term_panel {
                            let delta = if m.kind == MouseKind::WheelUp { 3 } else { -3 };
                            t.screen.scroll_view(delta);
                        }
                        return true;
                    }
                }
                let target_pid = self
                    .layout
                    .panes
                    .iter()
                    .find(|pl| pl.rect.contains(m.col, m.row))
                    .map(|pl| pl.pane_id)
                    .unwrap_or(self.active_pane);
                if self.panes.iter().any(|p| p.id == target_pid && p.active_view().md_preview) {
                    let delta = if m.kind == MouseKind::WheelUp { -3 } else { 3 };
                    self.scroll_md_pane(target_pid, delta, cols, rows);
                    return true;
                }
                // Shift + vertical wheel/touchpad scroll -> smooth horizontal pan (VS Code standard!)
                if m.shift && !self.cfg.wrap {
                    let delta = if m.kind == MouseKind::WheelUp { -6 } else { 6 };
                    self.scroll_pane_horizontal(target_pid, delta);
                    return true;
                }
                if self.cfg.wheel_moves_cursor {
                    // Move cursor 1 line per wheel tick (smooth line-by-line tracking with scrolloff),
                    // WITHOUT snapping horizontal scroll_col back to col 0!
                    self.focus_pane(target_pid);
                    self.follow_cursor = true;
                    self.follow_cursor_h = false;
                    let motion = if m.kind == MouseKind::WheelUp {
                        Motion::Up
                    } else {
                        Motion::Down
                    };
                    self.doc_mut().move_cursors(motion, m.shift);
                } else if target_pid == self.active_pane {
                    // Page/viewport scrolling without moving the cursor
                    self.follow_cursor = false;
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
                true
            }
            MouseKind::LeftDown => {
                self.clear_completion();
                // 0. Check integrated terminal header/divider/close button & body
                if let Some((header_row, body_h)) = self.term_layout(rows) {
                    if m.row == header_row {
                        if cols >= 3 && m.col >= cols - 3 {
                            self.kill_terminal();
                        } else {
                            self.drag = DragState::TermDivider;
                            self.hover_term_divider = true;
                            self.hover_term_close = false;
                            self.hover_shape = PointerShape::ResizeV;
                            self.set_status("Arraste ↕ para redimensionar o terminal");
                        }
                        return true;
                    }
                    if m.row > header_row && m.row < header_row + 1 + body_h {
                        self.term_focused = true;
                        self.mode = Mode::Edit;
                        self.drag = DragState::None;
                        return true;
                    }
                }
                // Clicking anywhere above the terminal returns focus to editor panes
                self.term_focused = false;

                // 1. Check split dividers (exact 1-col `│` / 1-row `─`)
                if let Some(div) = self.layout.dividers.iter().copied().find(|d| d.hits(m.col, m.row)) {
                    self.drag = DragState::Divider(div);
                    self.hover_divider = Some(div.split_id);
                    self.hover_scrollbar = None;
                    self.hover_shape = match div.axis {
                        SplitAxis::Horizontal => PointerShape::ResizeH,
                        SplitAxis::Vertical => PointerShape::ResizeV,
                    };
                    self.mode = Mode::Edit;
                    self.set_status("Arraste para redimensionar a divisoria");
                    return true;
                }

                // 2. Check pane scrollbars (clickable & draggable even in keyboard-only text nav mode!)
                let hit_sb = self.layout.panes.iter().find_map(|pl| {
                    if pl.scrollbar_col == Some(m.col)
                        && m.row >= pl.content_start_row
                        && m.row < pl.content_start_row + pl.content_height
                    {
                        Some((pl.pane_id, pl.content_start_row, pl.content_height))
                    } else {
                        None
                    }
                });
                if let Some((pid, start_row, height)) = hit_sb {
                    self.mode = Mode::Edit;
                    self.drag = DragState::Scrollbar {
                        pane_id: pid,
                        start_row,
                        height,
                    };
                    self.hover_scrollbar = Some(pid);
                    self.hover_shape = PointerShape::Grab;
                    self.scroll_pane_from_scrollbar(pid, m.row, start_row, height);
                    return true;
                }

                // 2b. Check bottom horizontal scrollbars (click/drag track or click ◀ / ▶ arrows)
                let hit_hsb = self.layout.panes.iter().find_map(|pl| {
                    let (hrow, start_col, track_w, max_sc) = pl.hscrollbar?;
                    if m.row == hrow && m.col >= pl.rect.x && m.col < pl.rect.x + pl.rect.w {
                        Some((pl.pane_id, start_col, track_w, max_sc))
                    } else {
                        None
                    }
                });
                if let Some((pid, start_col, track_w, max_sc)) = hit_hsb {
                    self.mode = Mode::Edit;
                    self.hover_hscrollbar = Some(pid);
                    self.hover_shape = PointerShape::Grab;
                    if m.col < start_col {
                        self.scroll_pane_horizontal(pid, -8);
                        self.drag = DragState::None;
                    } else if m.col >= start_col + track_w {
                        self.scroll_pane_horizontal(pid, 8);
                        self.drag = DragState::None;
                    } else {
                        self.drag = DragState::HScrollbar {
                            pane_id: pid,
                            start_col,
                            track_w,
                            max_scroll_col: max_sc,
                        };
                        self.scroll_pane_from_hscrollbar(pid, m.col, start_col, track_w, max_sc);
                    }
                    return true;
                }

                // 3. Check pane tab bars -> focus/switch immediately AND arm DragState::Tab for drag-and-drop!
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
                        self.hover_shape = PointerShape::Grab;
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
                    return true;
                }

                // 4. Pane editor canvas
                let hit_pane = self
                    .layout
                    .panes
                    .iter()
                    .find(|pl| pl.rect.contains(m.col, m.row))
                    .map(|pl| pl.pane_id);
                if let Some(pid) = hit_pane {
                    self.focus_pane(pid);
                    self.mode = Mode::Edit;
                    if self.pane(pid).active_view().md_preview {
                        let now = Instant::now();
                        let clicks = if let Some((prev_t, pr, pc, cnt)) = self.last_click {
                            if now.duration_since(prev_t).as_millis() < 400
                                && pr == m.row
                                && pc.abs_diff(m.col) <= 1
                            {
                                (cnt % 2) + 1
                            } else {
                                1
                            }
                        } else {
                            1
                        };
                        self.last_click = Some((now, m.row, m.col, clicks));
                        if clicks == 2 {
                            let src_row = self
                                .layout
                                .panes
                                .iter()
                                .find(|pl| pl.pane_id == pid)
                                .and_then(|pl| {
                                    let rel_y = m.row.checked_sub(pl.content_start_row)?;
                                    pl.md_row_sources.get(rel_y).copied()
                                })
                                .unwrap_or(0);
                            self.pane_mut(pid).active_view_mut().md_preview = false;
                            self.doc_mut().goto_line(src_row + 1);
                            self.follow_cursor = true;
                            self.set_status(format!("Modo Edicao (Ln {})", src_row + 1));
                        }
                        self.drag = DragState::None;
                        return true;
                    }
                    self.follow_cursor = true;
                    self.follow_cursor_h = true;
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
                                self.arm_trail();
                                self.doc_mut().set_cursor_at_line_col(doc_row, doc_col, m.shift);
                                self.drag = DragState::Selection(pid);
                            }
                        }
                    }
                    return true;
                }
                false
            }
            MouseKind::LeftDrag => match self.drag {
                DragState::TermDivider => {
                    let usable_h = rows.saturating_sub(self.bottom_reserved_rows());
                    let min_editor_h = (MIN_PANE_H + 1).max(4);
                    let max_header = usable_h.saturating_sub(4).max(min_editor_h);
                    let new_header = m.row.clamp(min_editor_h, max_header);
                    let new_body = usable_h.saturating_sub(new_header + 1).max(3);
                    self.term_height = new_body;
                    if let Some(ref mut t) = self.term_panel {
                        t.resize(cols, new_body);
                    }
                    self.hover_shape = PointerShape::ResizeV;
                    self.set_status(format!("Altura do terminal: {} linhas", new_body));
                    true
                }
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
                    true
                }
                DragState::Scrollbar {
                    pane_id,
                    start_row,
                    height,
                } => {
                    self.scroll_pane_from_scrollbar(pane_id, m.row, start_row, height);
                    true
                }
                DragState::HScrollbar {
                    pane_id,
                    start_col,
                    track_w,
                    max_scroll_col,
                } => {
                    self.scroll_pane_from_hscrollbar(pane_id, m.col, start_col, track_w, max_scroll_col);
                    true
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
                    self.hover_shape = PointerShape::Grab;
                    self.drag = DragState::Tab {
                        src_pane,
                        src_tab,
                        start_col,
                        start_row,
                        cur_col: m.col,
                        cur_row: m.row,
                        dragging,
                    };
                    dragging
                }
                DragState::Selection(pid) => {
                    if pid == self.active_pane {
                        if let Some((doc_row, doc_col)) = self.screen_to_doc_in_pane(pid, m.col, m.row) {
                            self.doc_mut().set_cursor_at_line_col(doc_row, doc_col, true);
                            return true;
                        }
                    }
                    false
                }
                DragState::None => false,
            },
            MouseKind::LeftUp => {
                let was_dragging = !matches!(self.drag, DragState::None);
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
                let hover_changed = self.update_hover(m.col, m.row, cols, rows);
                was_dragging || hover_changed
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

        // ---- Integrated Terminal Focus Mode (VS Code Ctrl+' bottom drawer) ----
        if self.term_open && self.term_focused {
            if self.term_panel.is_some() {
                // Global exit/navigation shortcuts from inside the integrated terminal
                if k.ctrl && !k.alt && !k.shift {
                    match k.code {
                        KeyCode::Char('\'') | KeyCode::Char('`') => {
                            self.toggle_terminal(cols, rows);
                            return;
                        }
                        KeyCode::Char(c @ '1'..='9') => {
                            self.term_focused = false;
                            let num = (c as u8 - b'0') as usize;
                            self.focus_pane_by_number(num, cols, rows);
                            return;
                        }
                        _ => {}
                    }
                }
                if k.shift && !k.ctrl && !k.alt {
                    let step = self
                        .term_layout(rows)
                        .map(|(_, h)| (h / 2).max(2) as isize)
                        .unwrap_or(5);
                    match k.code {
                        KeyCode::PageUp => {
                            if let Some(ref mut t) = self.term_panel {
                                t.screen.scroll_view(step);
                            }
                            return;
                        }
                        KeyCode::PageDown => {
                            if let Some(ref mut t) = self.term_panel {
                                t.screen.scroll_view(-step);
                            }
                            return;
                        }
                        _ => {}
                    }
                }
                if !k.ctrl && !k.alt && !k.shift && k.code == KeyCode::F(1) {
                    self.term_focused = false;
                    self.open_command_palette();
                    return;
                }
                if let Some(ref mut t) = self.term_panel {
                    t.send_key(k);
                }
                return;
            } else {
                self.term_focused = false;
                self.term_open = false;
            }
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
            if self.active_is_md_preview() {
                match k.code {
                    KeyCode::Char('v') => {
                        self.toggle_markdown_preview(cols, rows);
                        return;
                    }
                    KeyCode::Char('p') => {
                        self.open_command_palette();
                        return;
                    }
                    KeyCode::Char('o') => {
                        self.open_symbol_picker();
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
                    _ => return,
                }
            }
            match k.code {
                KeyCode::Char('v') => {
                    self.toggle_markdown_preview(cols, rows);
                    return;
                }
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
                KeyCode::Char('o') => {
                    self.open_symbol_picker();
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
                    self.arm_trail();
                    self.doc_mut().move_cursors(Motion::WordLeft, true);
                    return;
                }
                KeyCode::Right => {
                    self.arm_trail();
                    self.doc_mut().move_cursors(Motion::WordRight, true);
                    return;
                }
                KeyCode::Home => {
                    self.arm_trail();
                    self.doc_mut().move_cursors(Motion::DocStart, true);
                    return;
                }
                KeyCode::End => {
                    self.arm_trail();
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
                KeyCode::Char('m') => {
                    self.clear_completion();
                    let bc = self.cfg.block_cursor;
                    if self.doc_mut().jump_to_matching_bracket(bc) {
                        self.arm_trail();
                    } else {
                        self.set_status("Posicione o cursor em um delimitador (), [] ou {}");
                    }
                    return;
                }
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
                        self.arm_trail();
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
                        self.arm_trail();
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
                    let st = if self.cfg.vim_visual { "ON (relnum + scroll cursor)" } else { "OFF (VS Code padrao)" };
                    self.set_status(format!("Estilo Vim Visual: {}", st));
                    return;
                }
                KeyCode::Char('o') => {
                    self.apply_wt_bg_mode(None);
                    return;
                }
                KeyCode::Char('p') => {
                    self.toggle_markdown_preview(cols, rows);
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
                        self.arm_trail();
                        self.set_status(format!("Git hunk {}/{} (Ln {})", idx, tot, ln));
                    } else {
                        self.set_status("Nenhuma alteracao Git neste arquivo");
                    }
                    return;
                }
                KeyCode::Char('[') => {
                    if let Some((idx, tot, ln)) = self.doc_mut().jump_git_hunk(false) {
                        self.arm_trail();
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
                KeyCode::Char('\'') | KeyCode::Char('`') => {
                    self.toggle_terminal(cols, rows);
                    return;
                }
                KeyCode::Char('k') => {
                    self.mode = Mode::ChordK;
                    return;
                }
                KeyCode::Char(c @ '1'..='9') => {
                    let num = (c as u8 - b'0') as usize;
                    self.focus_pane_by_number(num, cols, rows);
                    return;
                }
                KeyCode::Char('s') => {
                    self.try_save_active();
                    return;
                }
                KeyCode::Char('q') => {
                    if self.cfg.auto_save {
                        self.flush_auto_save();
                    }
                    if self.docs.iter().flatten().any(|d| d.is_dirty()) {
                        self.mode = Mode::ConfirmQuit;
                    } else {
                        self.should_quit = true;
                    }
                    return;
                }
                KeyCode::Char('w') => {
                    let did = self.active_doc_id();
                    if self.cfg.auto_save && self.doc().is_dirty() && self.doc_ref_count(did) == 1 {
                        self.flush_auto_save();
                    }
                    if self.doc().is_dirty() && self.doc_ref_count(did) == 1 {
                        self.mode = Mode::ConfirmCloseTab;
                    } else {
                        self.close_active_tab_force();
                    }
                    return;
                }
                KeyCode::Char(' ') => {
                    self.refresh_completion(true);
                    if self.completion.is_none() {
                        self.set_status("Nenhuma sugestao neste ponto");
                    }
                    return;
                }
                KeyCode::Char(']') => {
                    self.clear_completion();
                    let bc = self.cfg.block_cursor;
                    if self.doc_mut().jump_to_matching_bracket(bc) {
                        self.arm_trail();
                    } else {
                        self.set_status("Posicione o cursor em um delimitador (), [] ou {}");
                    }
                    return;
                }
                KeyCode::Char('n') if self.completion_popup_visible() => {
                    if let Some(ref mut c) = self.completion {
                        c.move_sel(1);
                    }
                    return;
                }
                KeyCode::Char('p') if self.completion_popup_visible() => {
                    if let Some(ref mut c) = self.completion {
                        c.move_sel(-1);
                    }
                    return;
                }
                KeyCode::Char('n') => {
                    self.new_blank_tab_in_active_pane();
                    self.set_status("Nova aba criada");
                    return;
                }
                KeyCode::Char('p') => {
                    self.pending_pane_pickers.clear();
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
                    let txt = if self.active_is_md_preview() {
                        let d = self.doc();
                        d.rope.slice(0, d.rope.len_bytes())
                    } else {
                        self.doc().copy_text()
                    };
                    if !txt.is_empty() {
                        self.clipboard = txt.clone();
                        copy_to_system_clipboard(&txt);
                        self.set_status(format!("Copiado ({} chars)", txt.chars().count()));
                    }
                    return;
                }
                KeyCode::Char('x') => {
                    if self.active_is_md_preview() {
                        return;
                    }
                    let txt = self.doc_mut().cut_text();
                    if !txt.is_empty() {
                        self.clipboard = txt.clone();
                        copy_to_system_clipboard(&txt);
                        self.set_status(format!("Recortado ({} chars)", txt.chars().count()));
                    }
                    return;
                }
                KeyCode::Char('v') => {
                    if self.active_is_md_preview() {
                        return;
                    }
                    let txt = paste_from_system_clipboard().unwrap_or_else(|| self.clipboard.clone());
                    if !txt.is_empty() {
                        self.doc_mut().paste(&txt);
                    }
                    return;
                }
                KeyCode::Char('/') => {
                    if self.active_is_md_preview() {
                        return;
                    }
                    self.doc_mut().toggle_comment();
                    return;
                }
                KeyCode::Left => {
                    self.arm_trail();
                    self.doc_mut().move_cursors(Motion::WordLeft, false);
                    return;
                }
                KeyCode::Right => {
                    self.arm_trail();
                    self.doc_mut().move_cursors(Motion::WordRight, false);
                    return;
                }
                KeyCode::Up => {
                    self.arm_trail();
                    self.doc_mut().move_cursors(Motion::PageUp(5), false);
                    return;
                }
                KeyCode::Down => {
                    self.arm_trail();
                    self.doc_mut().move_cursors(Motion::PageDown(5), false);
                    return;
                }
                KeyCode::Home => {
                    self.arm_trail();
                    self.doc_mut().move_cursors(Motion::DocStart, false);
                    return;
                }
                KeyCode::End => {
                    self.arm_trail();
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
                if self.completion.is_some() {
                    self.completion_dismissed_byte = Some(self.doc().primary_cursor().head);
                    self.completion = None;
                } else if self.judge.is_running() {
                    self.judge.cancel();
                    self.set_status("Cancelando execucao...");
                } else if !matches!(self.drag, DragState::None) {
                    self.drag = DragState::None;
                    self.set_status("Arrasto cancelado");
                } else if !self.doc_mut().collapse_cursors() && !self.doc().search.query.is_empty() {
                    self.doc_mut().search.query.clear();
                    self.doc_mut().search.matches.clear();
                    self.doc_mut().search.active_idx = None;
                }
            }
            KeyCode::F(1) => {
                self.clear_completion();
                self.open_command_palette();
            }
            KeyCode::F(2) => {
                let next_idx = palette::PALETTES
                    .iter()
                    .position(|p| p.name == self.palette.name)
                    .map(|i| (i + 1) % palette::PALETTES.len())
                    .unwrap_or(0);
                self.set_palette_index(next_idx);
            }
            KeyCode::F(3) => {
                self.clear_completion();
                let m = if k.shift {
                    self.doc_mut().search.prev_match()
                } else {
                    self.doc_mut().search.next_match()
                };
                if let Some((s, e)) = m {
                    self.arm_trail();
                    self.select_byte_range(s, e);
                }
            }
            KeyCode::F(4) => {
                self.clear_completion();
                if let Some((idx, tot, ln)) = self.doc_mut().jump_bookmark(!k.shift) {
                    self.arm_trail();
                    self.set_status(format!("◆ Bookmark {}/{} (Ln {})", idx, tot, ln));
                } else {
                    self.set_status("Nenhum bookmark (use Alt+B para marcar linha)");
                }
            }
            KeyCode::F(5) => {
                if k.shift {
                    if let Err(e) = self.doc_mut().reload_from_disk() {
                        self.set_status(format!("Erro ao recarregar: {}", e));
                    } else {
                        self.set_status("Recarregado do disco");
                    }
                } else {
                    self.run_judge(false);
                }
            }
            KeyCode::F(6) => {
                self.run_judge(true);
            }
            KeyCode::F(7) => {
                self.toggle_markdown_preview(cols, rows);
            }
            KeyCode::F(8) => {
                self.clear_completion();
                self.jump_next_diagnostic();
            }
            KeyCode::F(9) => {
                if let Err(e) = self.doc_mut().reload_from_disk() {
                    self.set_status(format!("Erro ao recarregar: {}", e));
                } else {
                    self.set_status("Recarregado do disco");
                }
            }
            _ if self.active_is_md_preview() && !k.ctrl && !k.alt => {
                let pid = self.active_pane;
                let half = (page_step / 2).max(3) as isize;
                let page = page_step.max(4) as isize;
                match k.code {
                    KeyCode::Up | KeyCode::Char('k') => self.scroll_md_pane(pid, -1, cols, rows),
                    KeyCode::Down | KeyCode::Char('j') => self.scroll_md_pane(pid, 1, cols, rows),
                    KeyCode::PageUp | KeyCode::Char('b') => self.scroll_md_pane(pid, -page, cols, rows),
                    KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_md_pane(pid, page, cols, rows),
                    KeyCode::Char('u') => self.scroll_md_pane(pid, -half, cols, rows),
                    KeyCode::Char('d') => self.scroll_md_pane(pid, half, cols, rows),
                    KeyCode::Home => {
                        self.pane_mut(pid).active_view_mut().md_scroll = 0;
                    }
                    KeyCode::Char('g') if !k.shift => {
                        self.pane_mut(pid).active_view_mut().md_scroll = 0;
                    }
                    KeyCode::End | KeyCode::Char('G') => self.scroll_md_pane(pid, isize::MAX / 4, cols, rows),
                    KeyCode::Char('g') if k.shift => self.scroll_md_pane(pid, isize::MAX / 4, cols, rows),
                    KeyCode::Char(']') | KeyCode::Char('n') => self.jump_md_heading(true, cols, rows),
                    KeyCode::Char('[') | KeyCode::Char('p') => self.jump_md_heading(false, cols, rows),
                    KeyCode::Char('e') | KeyCode::Enter | KeyCode::Esc => {
                        self.toggle_markdown_preview(cols, rows);
                    }
                    _ => {}
                }
            }
            KeyCode::Left => {
                self.clear_completion();
                self.doc_mut().move_cursors(Motion::Left, k.shift);
            }
            KeyCode::Right => {
                self.clear_completion();
                self.doc_mut().move_cursors(Motion::Right, k.shift);
            }
            KeyCode::Up => {
                if !k.shift && self.completion_popup_visible() {
                    if let Some(ref mut c) = self.completion {
                        c.move_sel(-1);
                    }
                } else {
                    self.clear_completion();
                    self.doc_mut().move_cursors(Motion::Up, k.shift);
                }
            }
            KeyCode::Down => {
                if !k.shift && self.completion_popup_visible() {
                    if let Some(ref mut c) = self.completion {
                        c.move_sel(1);
                    }
                } else {
                    self.clear_completion();
                    self.doc_mut().move_cursors(Motion::Down, k.shift);
                }
            }
            KeyCode::Home => {
                self.clear_completion();
                self.arm_trail();
                self.doc_mut().move_cursors(Motion::Home, k.shift);
            }
            KeyCode::End => {
                self.clear_completion();
                self.arm_trail();
                self.doc_mut().move_cursors(Motion::End, k.shift);
            }
            KeyCode::PageUp => {
                self.clear_completion();
                self.arm_trail();
                self.doc_mut().move_cursors(Motion::PageUp(page_step), k.shift);
            }
            KeyCode::PageDown => {
                self.clear_completion();
                self.arm_trail();
                self.doc_mut().move_cursors(Motion::PageDown(page_step), k.shift);
            }
            KeyCode::Enter => {
                if !k.shift && self.completion.as_ref().is_some_and(|c| c.user_navigated) {
                    self.accept_completion();
                } else {
                    self.clear_completion();
                    if k.shift {
                        self.doc_mut().insert_line_below();
                    } else {
                        self.doc_mut().insert_newline();
                    }
                }
            }
            KeyCode::Tab => {
                self.arm_trail();
                if !self.accept_completion() && !self.try_expand_exact_snippet_on_tab() {
                    self.doc_mut().indent(false);
                }
            }
            KeyCode::BackTab => {
                self.clear_completion();
                self.arm_trail();
                self.doc_mut().indent(true);
            }
            KeyCode::Backspace => {
                self.cursor_trails.clear();
                self.doc_mut().backspace(false);
                self.refresh_completion(false);
            }
            KeyCode::Delete => {
                self.cursor_trails.clear();
                self.clear_completion();
                self.doc_mut().delete(false);
            }
            KeyCode::Char(ch) if !k.ctrl && !k.alt => {
                self.cursor_trails.clear();
                self.doc_mut().insert_char(ch);
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == ':' || ch == '>' {
                    self.refresh_completion(false);
                } else {
                    self.clear_completion();
                }
            }
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
                self.focus_pane_by_number((c as u8 - b'0') as usize, cols, rows);
            }
            KeyCode::Char('t') | KeyCode::Char('\'') | KeyCode::Char('`') => {
                self.toggle_terminal(cols, rows);
            }
            KeyCode::Char('m') | KeyCode::Char('p') => {
                self.open_markdown_preview_side(cols, rows);
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
        self.pending_pane_pickers.clear();
        let pk = Picker::command_palette(&self.cfg, self.doc().eol.label(), config::wt_bg_mode());
        self.mode = Mode::Picker(pk);
    }

    fn apply_wt_bg_mode(&mut self, target: Option<&str>) {
        match config::set_wt_bg_mode(target) {
            Ok(config::WtBgMode::Frosted) => {
                self.set_status("✓ Fundo: Vidro Escuro (Frosted 100% · imune a tela branca · Alt+O cicla)");
            }
            Ok(config::WtBgMode::Solid) => {
                self.set_status("✓ Fundo: Obsidiana Solido (100% · Alt+O cicla)");
            }
            Ok(config::WtBgMode::Glass(op)) => {
                self.set_status(format!(
                    "✓ Fundo: Translucido Real (Acrylic {}% · Alt+O cicla)",
                    op
                ));
            }
            Err(_) => {
                // Non-WSL / non-Windows-Terminal fallback: toggle solid editor canvas
                self.cfg.opaque_bg = !self.cfg.opaque_bg;
                self.cfg.save();
                let st = if self.cfg.opaque_bg { "Solido (opaco)" } else { "Translucido (herda terminal)" };
                self.set_status(format!("Fundo do Editor: {}", st));
            }
        }
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
                    self.arm_trail();
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
                    self.open_next_pending_pane_picker();
                    return;
                }
                KeyCode::Up if k.alt => {
                    pk.go_parent();
                    return;
                }
                KeyCode::Left if pk.query.is_empty() && pk.browse_dir.is_some() => {
                    pk.go_parent();
                    return;
                }
                KeyCode::Right if pk.query.is_empty() && pk.browse_dir.is_some() => {
                    if let Some(PickerAction::BrowseDir(dir)) = pk.chosen() {
                        pk.browse_to(&dir);
                    }
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
                KeyCode::Enter => {
                    if let Some(PickerAction::BrowseDir(dir)) = pk.chosen() {
                        pk.browse_to(&dir);
                        return;
                    }
                    pk.chosen()
                }
                _ => return,
            }
        } else {
            return;
        };

        self.mode = Mode::Edit;
        if let Some(act) = action {
            match act {
                PickerAction::OpenFile(path) => self.open_or_focus_file(&path, None),
                PickerAction::BrowseDir(_) => {}
                PickerAction::JumpToLine(ln) => {
                    if self.active_is_md_preview() {
                        let pid = self.active_pane;
                        let (text_w, _) = self.pane_md_geometry(pid, cols, rows);
                        let rendered = markdown::render_markdown(&self.doc().rope, text_w, self.palette);
                        let vrow = rendered.visual_row_for_source(ln.saturating_sub(1));
                        self.pane_mut(pid).active_view_mut().md_scroll = vrow;
                        self.set_status(format!("◈ Secao na linha {}", ln));
                    } else {
                        self.arm_trail();
                        self.doc_mut().goto_line(ln);
                        self.set_status(format!("Simbolo na linha {}", ln));
                    }
                }
                PickerAction::InsertSnippet(body) => self.insert_snippet_at_cursor(&body),
                PickerAction::ToggleAutocomplete => {
                    self.cfg.autocomplete = !self.cfg.autocomplete;
                    self.cfg.save();
                    if !self.cfg.autocomplete {
                        self.clear_completion();
                    }
                    let st = if self.cfg.autocomplete { "ON" } else { "OFF" };
                    self.set_status(format!("Autocomplete / Dicas: {}", st));
                }
                PickerAction::ToggleAutocompletePopup => {
                    self.cfg.autocomplete_popup = !self.cfg.autocomplete_popup;
                    self.cfg.save();
                    let st = if self.cfg.autocomplete_popup {
                        "ON (popup flutuante visivel)"
                    } else {
                        "OFF (so texto fantasma inline · Ctrl+Space abre popup)"
                    };
                    self.set_status(format!("Popup de Dicas do Autocomplete: {}", st));
                }
                PickerAction::CycleWtBgMode => self.apply_wt_bg_mode(None),
                PickerAction::SetWtBgFrosted => self.apply_wt_bg_mode(Some("frosted")),
                PickerAction::SetWtBgSolid => self.apply_wt_bg_mode(Some("solid")),
                PickerAction::SetWtBgGlass => self.apply_wt_bg_mode(Some("glass")),
                PickerAction::ToggleOpaqueBg => {
                    self.cfg.opaque_bg = !self.cfg.opaque_bg;
                    self.cfg.save();
                    let st = if self.cfg.opaque_bg {
                        "ON (canvas opaco puro)"
                    } else {
                        "OFF (herda fundo do terminal)"
                    };
                    self.set_status(format!("Fundo Solido no Editor: {}", st));
                }
                PickerAction::CycleAutocompleteMinLen => {
                    self.cfg.cycle_autocomplete_min_len();
                    self.set_status(format!(
                        "Autocomplete min_len: {} caracteres",
                        self.cfg.autocomplete_min_len
                    ));
                }
                PickerAction::ToggleCpRadar => {
                    self.cfg.cp_radar = !self.cfg.cp_radar;
                    self.cfg.save();
                    let st = if self.cfg.cp_radar { "ON" } else { "OFF" };
                    self.set_status(format!("Radar Anti-Armadilhas CP: {}", st));
                }
                PickerAction::ToggleExamMode => {
                    self.cfg.exam_mode = !self.cfg.exam_mode;
                    self.cfg.save();
                    self.clear_completion();
                    let st = if self.cfg.exam_mode {
                        "ON (snippets prontos bloqueados)"
                    } else {
                        "OFF"
                    };
                    self.set_status(format!("Modo Prova / ICPC: {}", st));
                }
                PickerAction::JudgeRunRelease => self.run_judge(false),
                PickerAction::JudgeRunDebug => self.run_judge(true),
                PickerAction::NextDiagnostic => self.jump_next_diagnostic(),
                PickerAction::CpLayoutSetup => self.setup_cp_layout(cols, rows),
                PickerAction::OpenSnippetsPicker => self.open_snippets_picker(),
                PickerAction::StressTestRun => self.run_stress_test(),
                PickerAction::StressTestScaffold => self.scaffold_stress_in_current_dir(),
                PickerAction::StressTestCancel => {
                    self.judge.cancel();
                    self.set_status("Execucao do Judge/Stress cancelada");
                }
                PickerAction::GotoSymbol => self.open_symbol_picker(),
                PickerAction::JumpMatchingBracket => {
                    let bc = self.cfg.block_cursor;
                    if !self.doc_mut().jump_to_matching_bracket(bc) {
                        self.set_status("Posicione o cursor em um delimitador (), [] ou {}");
                    }
                }
                PickerAction::CopyLeetCodeSolution => self.copy_leetcode_solution(),
                PickerAction::TimerStopwatch => {
                    self.cp_timer.start_stopwatch();
                    self.set_status("⏱ Cronometro CP iniciado");
                }
                PickerAction::TimerCountdown(mins) => {
                    self.cp_timer.start_countdown(mins);
                    self.set_status(format!("⏳ Contagem regressiva CP: {} min iniciada", mins));
                }
                PickerAction::TimerTogglePause => {
                    self.cp_timer.toggle_pause();
                    let st = if self.cp_timer.running { "retomado" } else { "pausado" };
                    self.set_status(format!("Timer CP {}", st));
                }
                PickerAction::TimerStop => {
                    self.cp_timer.stop();
                    self.set_status("Timer CP desligado");
                }
                PickerAction::ToggleVimVisual => {
                    self.cfg.toggle_vim_visual();
                    self.term_sync_needed = true;
                    let st = if self.cfg.vim_visual { "ON (relnum + scroll cursor)" } else { "OFF (VS Code padrao)" };
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
                PickerAction::ToggleCursorTrail => {
                    self.cfg.cursor_trail = !self.cfg.cursor_trail;
                    self.cfg.save();
                    if !self.cfg.cursor_trail {
                        self.cursor_trails.clear();
                    }
                    let st = if self.cfg.cursor_trail { "ON" } else { "OFF" };
                    self.set_status(format!("Animacao de Trajeto do Cursor: {}", st));
                }
                PickerAction::ToggleAutoHideMouse => {
                    self.cfg.auto_hide_mouse = !self.cfg.auto_hide_mouse;
                    self.cfg.save();
                    self.term_sync_needed = true;
                    let st = if self.cfg.auto_hide_mouse { "ON (sempre ativo)" } else { "OFF" };
                    self.set_status(format!("Auto-Hide Ponteiro do Mouse: {}", st));
                }
                PickerAction::ToggleWheelMovesCursor => {
                    self.cfg.wheel_moves_cursor = !self.cfg.wheel_moves_cursor;
                    self.cfg.save();
                    let st = if self.cfg.wheel_moves_cursor {
                        "ON (move o cursor linha a linha)"
                    } else {
                        "OFF (rola a pagina)"
                    };
                    self.set_status(format!("Scroll do Mouse: {}", st));
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
                PickerAction::ToggleAutoSave => {
                    self.cfg.auto_save = !self.cfg.auto_save;
                    self.cfg.save();
                    if self.cfg.auto_save {
                        self.flush_auto_save();
                        self.set_status("✓ Auto-Save: ON (salva automaticamente apos 600ms)");
                    } else {
                        self.pending_autosave_at = None;
                        self.set_status("Auto-Save: OFF (salve manualmente com Ctrl+S)");
                    }
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
                PickerAction::ToggleTerminal => self.toggle_terminal(cols, rows),
                PickerAction::RestartTerminal => self.restart_terminal(cols, rows),
                PickerAction::ToggleMarkdownPreview => self.toggle_markdown_preview(cols, rows),
                PickerAction::MarkdownPreviewSide => self.open_markdown_preview_side(cols, rows),
                PickerAction::SetPalette(idx) => self.set_palette_index(idx),
            }
        }
        if matches!(self.mode, Mode::Edit) {
            self.open_next_pending_pane_picker();
        }
    }

    fn set_palette_index(&mut self, idx: usize) {
        self.palette = Palette::by_index(idx);
        self.cfg.palette = self.palette.name.to_string();
        self.cfg.save();
        self.term_sync_needed = true;
        self.set_status(format!("Paleta: {}", self.palette.name));
    }

    fn dirty_edit_signature(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for (did, slot) in self.docs.iter().enumerate() {
            if Some(did) == self.judge_doc_id {
                continue;
            }
            if let Some(doc) = slot.as_ref() {
                if doc.is_dirty() {
                    h ^= (did as u64).wrapping_add(1).wrapping_mul(0x9e3779b97f4a7c15);
                    h = h.wrapping_mul(0x100000001b3) ^ (doc.revision() as u64);
                }
            }
        }
        h
    }

    fn flush_auto_save(&mut self) -> bool {
        let mut saved_any = false;
        let mut last_err: Option<String> = None;
        let jid = self.judge_doc_id;
        for did in 0..self.docs.len() {
            if Some(did) == jid {
                continue;
            }
            if let Some(doc) = self.docs[did].as_mut() {
                if doc.is_dirty() && doc.path.is_some() && !doc.external_changed {
                    match doc.save() {
                        Ok(()) => saved_any = true,
                        Err(e) => last_err = Some(e.to_string()),
                    }
                }
            }
        }
        self.pending_autosave_at = None;
        self.last_edit_sig = self.dirty_edit_signature();
        if let Some(e) = last_err {
            self.set_status(format!("Auto-Save falhou: {}", e));
        }
        saved_any
    }

    fn poll_auto_save(&mut self) -> bool {
        let sig = self.dirty_edit_signature();
        if sig != self.last_edit_sig {
            self.last_edit_sig = sig;
            if self.cfg.auto_save && sig != 0xcbf29ce484222325 {
                self.pending_autosave_at = Some(Instant::now());
            } else {
                self.pending_autosave_at = None;
            }
            return false;
        }
        if !self.cfg.auto_save {
            return false;
        }
        if self
            .pending_autosave_at
            .is_some_and(|t| t.elapsed() >= Duration::from_millis(600))
        {
            return self.flush_auto_save();
        }
        false
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

        // Keep active pane's cursor visible along vertical/horizontal axes independently
        if (self.follow_cursor || self.follow_cursor_h) && !self.active_is_md_preview() {
            if let Some(active_rect) = computed.rect_of(self.active_pane) {
                let (content_h, _) = {
                    let d = self.doc();
                    pane_content_height(d, active_rect.w, active_rect.h, d.scroll_row, d.scroll_col, &cfg)
                };
                let fv = self.follow_cursor;
                let fh = self.follow_cursor_h;
                ensure_cursor_visible(self.doc_mut(), active_rect.w, content_h.max(1), &cfg, fv, fh);
            }
        }
        self.sync_active_to_tab();

        // Live Split Preview auto-sync: when editing a .md on one pane and another pane previews it,
        // keep the preview gently aligned with the edited/focused source line!
        if !self.active_is_md_preview() && self.follow_cursor {
            let active_did = self.active_doc_id();
            let (prow, _) = self.doc().primary_line_col();
            let rev = self.doc().revision();
            let sig = (active_did, prow, rev);
            if sig != self.last_md_sync {
                self.last_md_sync = sig;
                let preview_pids: Vec<PaneId> = self
                    .panes
                    .iter()
                    .filter(|p| {
                        p.id != self.active_pane
                            && p.active_view().doc_id == active_did
                            && p.active_view().md_preview
                    })
                    .map(|p| p.id)
                    .collect();
                for ppid in preview_pids {
                    if let Some(prect) = computed.rect_of(ppid) {
                        let sb_w = if cfg.scrollbar && prect.w >= 16 { 1 } else { 0 };
                        let tw = prect.w.saturating_sub(sb_w).max(8);
                        let ph = prect.h.saturating_sub(1).max(1);
                        let rendered = markdown::render_markdown(&self.doc().rope, tw, self.palette);
                        let vrow = rendered.visual_row_for_source(prow);
                        let max_s = rendered.total_lines().saturating_sub(ph);
                        let target = vrow.saturating_sub(ph / 3).min(max_s);
                        self.pane_mut(ppid).active_view_mut().md_scroll = target;
                    }
                }
            }
        }

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
                            md_preview: tab.md_preview,
                        }
                    })
                    .collect();
                Some(PaneRenderInput {
                    pane_id: pid,
                    group_num: order_idx + 1,
                    rect,
                    is_focused: pid == self.active_pane && !(self.term_open && self.term_focused),
                    doc_id: tv.doc_id,
                    tabs,
                    scroll_row: if tv.md_preview { tv.md_scroll } else { tv.scroll_row },
                    scroll_col: if tv.md_preview { 0 } else { tv.scroll_col },
                    cursors: tv.cursors.clone(),
                    primary: tv.primary,
                    md_preview: tv.md_preview,
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
                    "Ctrl+K → [\\] lado a lado · [-] topo/base · [m] preview .md ao lado · [←↑↓→] focar · [o] inverter · [s] trocar · [r] redimensionar · [=] 50/50 · [w] fechar",
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
        let hot_scrollbar = match self.drag {
            DragState::Scrollbar { pane_id, .. } => Some(pane_id),
            _ => self.hover_scrollbar,
        };
        let hot_hscrollbar = match self.drag {
            DragState::HScrollbar { pane_id, .. } => Some(pane_id),
            _ => self.hover_hscrollbar,
        };
        let highlight_all_dividers = matches!(self.mode, Mode::ResizePane);

        // Keep integrated PTY dimensions synced with live drawer geometry
        let term_geom = self.term_layout(rows);
        if let Some((_, body_h)) = term_geom {
            if let Some(ref mut t) = self.term_panel {
                t.resize(cols, body_h);
            }
        }
        let term_view = term_geom.and_then(|(header_row, body_h)| {
            self.term_panel.as_ref().map(|t| TermPanelView {
                screen: &t.screen,
                header_row,
                body_height: body_h,
                is_focused: self.term_focused,
                hover_divider: self.hover_term_divider,
                dragging_divider: matches!(self.drag, DragState::TermDivider),
                hover_close: self.hover_term_close,
                cwd_label: &t.cwd_label,
                shell_name: &t.shell_name,
            })
        });
        let term_bg_alive = self.term_panel.as_ref().is_some_and(|t| t.alive);
        let active_diags: Vec<Diagnostic> = self
            .active_doc_key()
            .and_then(|k| self.diagnostics.get(&k).cloned())
            .unwrap_or_default();
        let timer_badge = self.cp_timer.badge();
        let judge_progress = self.judge.current_progress();

        // Surface current line's compiler diagnostic or CP Pitfall Radar warning if no status msg active
        let cur_line_warning: Option<String> = if status.is_none() && mode_hint.is_none() && !(self.term_open && self.term_focused) {
            let (cur_r, _) = self.doc().primary_line_col();
            if let Some(d) = active_diags.iter().find(|d| d.line == cur_r) {
                Some(format!("{} (F8 prox)", d.message))
            } else if cfg.cp_radar && self.doc().lang == syntax::Language::CLike {
                let has_hash = file_has_custom_hash(&self.doc().rope);
                let ltxt = self.doc().rope.line(cur_r);
                detect_cp_pitfall(&ltxt, self.doc().lang, has_hash).map(str::to_string)
            } else {
                None
            }
        } else {
            None
        };
        let eff_mode_hint = mode_hint.or(cur_line_warning.as_deref());
        let comp_ref = if matches!(self.mode, Mode::Edit) && !(self.term_open && self.term_focused) {
            self.completion.as_ref()
        } else {
            None
        };

        // Launch cursor travel trail on frame 0 when an explicit jump/click/pane-switch occurred
        let now_frame = Instant::now();
        let predicted_pos = pane_views
            .iter()
            .find(|p| p.is_focused)
            .and_then(|p| {
                self.docs
                    .get(p.doc_id)
                    .and_then(|d| d.as_ref())
                    .and_then(|doc| predict_primary_screen_pos(doc, p, &cfg))
            });
        if self.arm_cursor_trail {
            self.arm_cursor_trail = false;
            if cfg.cursor_trail && matches!(self.mode, Mode::Edit) && !(self.term_open && self.term_focused) {
                // If a previous shooting star is still mid-flight, chain smoothly from its live head!
                let live_from = self
                    .cursor_trails
                    .last()
                    .filter(|tr| tr.is_head_flying(now_frame))
                    .and_then(|tr| tr.current_head_pos(now_frame))
                    .or(self.last_cursor_screen);
                if let (Some(from), Some(to)) = (live_from, predicted_pos) {
                    if let Some(tr) = CursorTrail::new(from, to, now_frame) {
                        self.cursor_trails.push(tr);
                        if self.cursor_trails.len() > 4 {
                            let excess = self.cursor_trails.len() - 4;
                            self.cursor_trails.drain(0..excess);
                        }
                    }
                }
            }
        }
        self.cursor_trails.retain(|tr| !tr.is_finished(now_frame));
        let trail_cells = if cfg.cursor_trail && !self.cursor_trails.is_empty() {
            build_trail_map(&self.cursor_trails, now_frame, self.palette)
        } else {
            HashMap::new()
        };
        let hide_editor_cursor = cfg.cursor_trail
            && self.cursor_trails.last().is_some_and(|tr| tr.is_head_flying(now_frame));

        let (frame, layout_info) = render_editor(
            &mut self.docs,
            &pane_views,
            &computed,
            active_divider,
            self.hover_divider,
            hot_scrollbar,
            hot_hscrollbar,
            highlight_all_dividers,
            drop_preview.as_ref(),
            term_view.as_ref(),
            term_bg_alive,
            comp_ref,
            &active_diags,
            timer_badge.as_ref(),
            judge_progress.as_deref(),
            &trail_cells,
            hide_editor_cursor,
            self.palette,
            cols,
            rows,
            &cfg,
            prompt,
            picker_ref,
            status.as_deref(),
            eff_mode_hint,
            synced,
        );
        if layout_info.primary_cursor_screen.is_some() {
            self.last_cursor_screen = layout_info.primary_cursor_screen;
        }
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
        "{}\n\nUso:\n  nave-edit [opcoes] [arquivo[:linha] ...]\n  nave-edit +<linha> <arquivo>\n\nOpcoes:\n  -h, --help          Mostra esta ajuda\n  -v, --version       Mostra versao\n  -p, --preview       Abre arquivo(s) .md direto no Modo Visualizacao (Reader)\n  --preview-split     Abre .md em Split Vivo (editor a esquerda | preview a direita)\n  --cp                Abre layout Maratona 3 paineis (codigo | in.txt / expected.txt)\n  -O                  Abre multiplos arquivos lado a lado (│)\n  -o                  Abre multiplos arquivos acima/abaixo (─)\n  --render-once <f>   Renderiza 1 frame estatico e sai\n\nAtalhos principais (VS Code + Multi-Janela + CP Suite + Markdown Reader):\n  Ctrl+S salvar · Ctrl+Q sair · Ctrl+W fechar aba · Ctrl+P abrir arquivo · F1 comandos\n  Alt+P / F7 preview .md · Ctrl+K m preview .md ao lado (split vivo) · [/] secoes .md\n  F5 compilar+julgar (O2) · F6 debug (ASan+UBSan) · F8 prox erro · Ctrl+Space autocomplete\n  Ctrl+Shift+O ir p/ simbolo/titulo · Ctrl+] / Alt+M pular par {{}} · Ctrl+' terminal integrado\n  Ctrl+\\ dividir lado a lado · Ctrl+K - dividir topo/base · Ctrl+1..9 focar/criar painel\n  Ctrl+PgUp/PgDn viajar abas · Alt+=/Alt+- redimensionar · Ctrl+D multi-cursor · Alt+1..6 paletas",
        VERSION
    );
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut render_once = false;
    let mut cp_layout = false;
    let mut start_preview = false;
    let mut start_preview_split = false;
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
            "-p" | "--preview" => {
                start_preview = true;
            }
            "--preview-split" => {
                start_preview_split = true;
            }
            "--cp" => {
                cp_layout = true;
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
    if cp_layout {
        let (cols, rows) = Terminal::size();
        app.setup_cp_layout(cols, rows);
    } else if start_preview_split {
        let (cols, rows) = Terminal::size();
        app.open_markdown_preview_side(cols, rows);
    } else if start_preview {
        for p in &mut app.panes {
            for tv in &mut p.tabs {
                tv.md_preview = true;
            }
        }
    }

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
    let mut esc_pending_since: Option<Instant> = None;
    let mut last_timer_tick: u64 = 0;
    let mut last_trail_frame = Instant::now();

    while !app.should_quit {
        let (cols, rows) = Terminal::size();

        if app.drain_terminal_output() {
            needs_redraw = true;
        }
        if app.poll_judge_updates(cols, rows) {
            needs_redraw = true;
        }
        if app.poll_auto_save() {
            needs_redraw = true;
        }
        if !app.cursor_trails.is_empty() && last_trail_frame.elapsed() >= Duration::from_millis(8) {
            needs_redraw = true;
            last_trail_frame = Instant::now();
        }
        if app.cp_timer.active && app.cp_timer.running {
            let cur_s = app.cp_timer.elapsed_secs();
            if cur_s != last_timer_tick {
                last_timer_tick = cur_s;
                needs_redraw = true;
            }
        }

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

        let poll_ms = if !app.cursor_trails.is_empty() {
            8
        } else if decoder.has_pending_esc() {
            15
        } else {
            60
        };
        let (stdin_ready, pty_ready) = term.poll_events(app.term_pty_fd(), poll_ms);
        if pty_ready && app.drain_terminal_output() {
            needs_redraw = true;
        }
        if stdin_ready {
            let n = term.read_bytes(&mut read_buf);
            if n == 0 {
                // Controlling TTY closed (EOF/HUP): exit cleanly instead of spinning
                break;
            }
            if n > 0 {
                decoder.push(&read_buf[..n as usize]);
                while let Some(ev) = decoder.next_event() {
                    let is_hover_move = matches!(
                        ev,
                        Event::Mouse(input::MouseEvent { kind: MouseKind::Move, .. })
                    );
                    if !is_hover_move {
                        last_activity = Instant::now();
                        if standby_blink {
                            standby_blink = false;
                            term.set_cursor_style(app.cfg.block_cursor, false);
                        }
                    }
                    if matches!(ev, Event::Key(_)) {
                        mouse_hider.notify_key();
                    }
                    if app.handle_event(ev, cols, rows) {
                        needs_redraw = true;
                    }
                    mouse_hider.set_shape(app.hover_shape);
                    if app.should_quit {
                        break;
                    }
                }
                if decoder.has_pending_esc() {
                    esc_pending_since.get_or_insert_with(Instant::now);
                } else {
                    esc_pending_since = None;
                }
            }
        } else if decoder.has_pending_esc()
            && esc_pending_since.is_none_or(|t| t.elapsed() >= Duration::from_millis(25))
        {
            esc_pending_since = None;
            if let Some(ev) = decoder.flush_bare_esc() {
                if app.handle_event(ev, cols, rows) {
                    needs_redraw = true;
                }
                mouse_hider.set_shape(app.hover_shape);
            }
        }
    }
}
