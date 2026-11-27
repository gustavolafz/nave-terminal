use std::collections::{HashMap, HashSet};

pub type PaneId = usize;

pub const MIN_PANE_W: usize = 16;
pub const MIN_PANE_H: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitAxis {
    /// Side-by-side: Left │ Right (vertical divider `│`)
    Horizontal,
    /// Stacked: Top ─ Bottom (horizontal divider `─`)
    Vertical,
}

impl SplitAxis {
    pub fn flipped(self) -> Self {
        match self {
            SplitAxis::Horizontal => SplitAxis::Vertical,
            SplitAxis::Vertical => SplitAxis::Horizontal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Rect {
    pub fn contains(self, col: usize, row: usize) -> bool {
        col >= self.x && col < self.x + self.w && row >= self.y && row < self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DividerHit {
    pub split_id: usize,
    pub axis: SplitAxis,
    pub x: usize,
    pub y: usize,
    pub len: usize,
    pub parent_rect: Rect,
}

impl DividerHit {
    pub fn hits(self, col: usize, row: usize) -> bool {
        match self.axis {
            SplitAxis::Horizontal => col == self.x && row >= self.y && row < self.y + self.len,
            SplitAxis::Vertical => row == self.y && col >= self.x && col < self.x + self.len,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ComputedLayout {
    pub panes: Vec<(PaneId, Rect)>,
    pub dividers: Vec<DividerHit>,
    pub junctions: Vec<(usize, usize, char)>,
}

impl ComputedLayout {
    pub fn rect_of(&self, pane_id: PaneId) -> Option<Rect> {
        self.panes.iter().find(|&&(id, _)| id == pane_id).map(|&(_, r)| r)
    }
}

#[derive(Clone, Debug)]
pub enum LayoutNode {
    Leaf(PaneId),
    Split {
        id: usize,
        axis: SplitAxis,
        /// Fraction [0.15 .. 0.85] allocated to `first` child
        ratio: f32,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

impl LayoutNode {
    pub fn leaf(id: PaneId) -> Self {
        LayoutNode::Leaf(id)
    }

    #[cfg(test)]
    pub fn pane_count(&self) -> usize {
        match self {
            LayoutNode::Leaf(_) => 1,
            LayoutNode::Split { first, second, .. } => first.pane_count() + second.pane_count(),
        }
    }

    pub fn pane_order(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect_order(&mut out);
        out
    }

    fn collect_order(&self, out: &mut Vec<PaneId>) {
        match self {
            LayoutNode::Leaf(id) => out.push(*id),
            LayoutNode::Split { first, second, .. } => {
                first.collect_order(out);
                second.collect_order(out);
            }
        }
    }

    pub fn contains(&self, target: PaneId) -> bool {
        match self {
            LayoutNode::Leaf(id) => *id == target,
            LayoutNode::Split { first, second, .. } => first.contains(target) || second.contains(target),
        }
    }

    pub fn compute(&self, area: Rect) -> ComputedLayout {
        let mut out = ComputedLayout::default();
        if area.w == 0 || area.h == 0 {
            return out;
        }
        self.compute_rec(area, &mut out);
        compute_junctions(&mut out);
        out
    }

    fn compute_rec(&self, area: Rect, out: &mut ComputedLayout) {
        match self {
            LayoutNode::Leaf(id) => {
                out.panes.push((*id, area));
            }
            LayoutNode::Split {
                id,
                axis,
                ratio,
                first,
                second,
            } => match axis {
                SplitAxis::Horizontal => {
                    if area.w < 3 {
                        first.compute_rec(area, out);
                        return;
                    }
                    let avail = area.w - 1;
                    let min_side = MIN_PANE_W.min(avail / 2).max(1);
                    let max_first = avail.saturating_sub(min_side).max(min_side);
                    let w1 = (((avail as f32) * ratio).round() as usize).clamp(min_side, max_first);
                    let w2 = avail - w1;
                    let r1 = Rect { x: area.x, y: area.y, w: w1, h: area.h };
                    let div_x = area.x + w1;
                    let r2 = Rect { x: div_x + 1, y: area.y, w: w2, h: area.h };
                    out.dividers.push(DividerHit {
                        split_id: *id,
                        axis: SplitAxis::Horizontal,
                        x: div_x,
                        y: area.y,
                        len: area.h,
                        parent_rect: area,
                    });
                    first.compute_rec(r1, out);
                    second.compute_rec(r2, out);
                }
                SplitAxis::Vertical => {
                    if area.h < 3 {
                        first.compute_rec(area, out);
                        return;
                    }
                    let avail = area.h - 1;
                    let min_side = MIN_PANE_H.min(avail / 2).max(1);
                    let max_first = avail.saturating_sub(min_side).max(min_side);
                    let h1 = (((avail as f32) * ratio).round() as usize).clamp(min_side, max_first);
                    let h2 = avail - h1;
                    let r1 = Rect { x: area.x, y: area.y, w: area.w, h: h1 };
                    let div_y = area.y + h1;
                    let r2 = Rect { x: area.x, y: div_y + 1, w: area.w, h: h2 };
                    out.dividers.push(DividerHit {
                        split_id: *id,
                        axis: SplitAxis::Vertical,
                        x: area.x,
                        y: div_y,
                        len: area.w,
                        parent_rect: area,
                    });
                    first.compute_rec(r1, out);
                    second.compute_rec(r2, out);
                }
            },
        }
    }

    pub fn split_pane(
        &mut self,
        target: PaneId,
        new_id: PaneId,
        split_id: usize,
        axis: SplitAxis,
        place_first: bool,
    ) -> bool {
        match self {
            LayoutNode::Leaf(id) if *id == target => {
                let old = Box::new(LayoutNode::Leaf(target));
                let added = Box::new(LayoutNode::Leaf(new_id));
                let (first, second) = if place_first { (added, old) } else { (old, added) };
                *self = LayoutNode::Split {
                    id: split_id,
                    axis,
                    ratio: 0.5,
                    first,
                    second,
                };
                true
            }
            LayoutNode::Leaf(_) => false,
            LayoutNode::Split { first, second, .. } => {
                first.split_pane(target, new_id, split_id, axis, place_first)
                    || second.split_pane(target, new_id, split_id, axis, place_first)
            }
        }
    }

    /// Remove `target` leaf and collapse its parent split. Returns a sibling PaneId for focus handoff.
    pub fn remove_pane(&mut self, target: PaneId) -> Option<PaneId> {
        match self {
            LayoutNode::Leaf(_) => None,
            LayoutNode::Split { first, second, .. } => {
                if matches!(first.as_ref(), LayoutNode::Leaf(id) if *id == target) {
                    let replacement = std::mem::replace(second.as_mut(), LayoutNode::Leaf(0));
                    let focus = replacement.first_leaf();
                    *self = replacement;
                    return Some(focus);
                }
                if matches!(second.as_ref(), LayoutNode::Leaf(id) if *id == target) {
                    let replacement = std::mem::replace(first.as_mut(), LayoutNode::Leaf(0));
                    let focus = replacement.last_leaf();
                    *self = replacement;
                    return Some(focus);
                }
                first.remove_pane(target).or_else(|| second.remove_pane(target))
            }
        }
    }

    fn first_leaf(&self) -> PaneId {
        match self {
            LayoutNode::Leaf(id) => *id,
            LayoutNode::Split { first, .. } => first.first_leaf(),
        }
    }

    fn last_leaf(&self) -> PaneId {
        match self {
            LayoutNode::Leaf(id) => *id,
            LayoutNode::Split { second, .. } => second.last_leaf(),
        }
    }

    /// Flip orientation (`Horizontal` <-> `Vertical`) of the nearest ancestor split containing `target`.
    pub fn toggle_axis(&mut self, target: PaneId) -> Option<SplitAxis> {
        match self {
            LayoutNode::Leaf(_) => None,
            LayoutNode::Split { axis, first, second, .. } => {
                if let Some(res) = first.toggle_axis(target) {
                    return Some(res);
                }
                if let Some(res) = second.toggle_axis(target) {
                    return Some(res);
                }
                if first.contains(target) || second.contains(target) {
                    *axis = axis.flipped();
                    return Some(*axis);
                }
                None
            }
        }
    }

    /// Swap `first` and `second` children of the nearest ancestor split containing `target`.
    pub fn swap_sibling(&mut self, target: PaneId) -> bool {
        match self {
            LayoutNode::Leaf(_) => false,
            LayoutNode::Split { first, second, .. } => {
                if first.swap_sibling(target) || second.swap_sibling(target) {
                    return true;
                }
                if first.contains(target) || second.contains(target) {
                    std::mem::swap(first, second);
                    return true;
                }
                false
            }
        }
    }

    /// Grow (`delta > 0`) or shrink (`delta < 0`) `target` within its nearest ancestor split.
    pub fn resize_pane_grow(&mut self, target: PaneId, delta: f32) -> bool {
        match self {
            LayoutNode::Leaf(_) => false,
            LayoutNode::Split { ratio, first, second, .. } => {
                if first.resize_pane_grow(target, delta) || second.resize_pane_grow(target, delta) {
                    return true;
                }
                if first.contains(target) {
                    *ratio = (*ratio + delta).clamp(0.15, 0.85);
                    return true;
                }
                if second.contains(target) {
                    *ratio = (*ratio - delta).clamp(0.15, 0.85);
                    return true;
                }
                false
            }
        }
    }

    /// Nudge the nearest ancestor split matching `wanted_axis` (`delta > 0` moves divider right/down).
    pub fn nudge_divider(&mut self, target: PaneId, wanted_axis: SplitAxis, delta: f32) -> bool {
        match self {
            LayoutNode::Leaf(_) => false,
            LayoutNode::Split { axis, ratio, first, second, .. } => {
                if first.nudge_divider(target, wanted_axis, delta)
                    || second.nudge_divider(target, wanted_axis, delta)
                {
                    return true;
                }
                if *axis == wanted_axis && (first.contains(target) || second.contains(target)) {
                    *ratio = (*ratio + delta).clamp(0.15, 0.85);
                    return true;
                }
                false
            }
        }
    }

    pub fn set_split_ratio(&mut self, target_split_id: usize, new_ratio: f32) -> bool {
        match self {
            LayoutNode::Leaf(_) => false,
            LayoutNode::Split { id, ratio, first, second, .. } => {
                if *id == target_split_id {
                    *ratio = new_ratio.clamp(0.15, 0.85);
                    return true;
                }
                first.set_split_ratio(target_split_id, new_ratio)
                    || second.set_split_ratio(target_split_id, new_ratio)
            }
        }
    }

    pub fn equalize_all(&mut self) {
        if let LayoutNode::Split { axis, ratio, first, second, .. } = self {
            let a = *axis;
            let n1 = first.axis_weight(a) as f32;
            let n2 = second.axis_weight(a) as f32;
            *ratio = (n1 / (n1 + n2)).clamp(0.15, 0.85);
            first.equalize_all();
            second.equalize_all();
        }
    }

    fn axis_weight(&self, target_axis: SplitAxis) -> usize {
        match self {
            LayoutNode::Leaf(_) => 1,
            LayoutNode::Split { axis, first, second, .. } => {
                if *axis == target_axis {
                    first.axis_weight(target_axis) + second.axis_weight(target_axis)
                } else {
                    1
                }
            }
        }
    }
}

pub fn find_neighbor(active: PaneId, dir: Dir, computed: &ComputedLayout) -> Option<PaneId> {
    let src = computed.rect_of(active)?;
    let mut best: Option<(usize, usize, PaneId)> = None; // (gap, neg_overlap, id)

    for &(id, dst) in &computed.panes {
        if id == active {
            continue;
        }
        let candidate = match dir {
            Dir::Right if dst.x >= src.x + src.w => {
                let ov = overlap_1d(src.y, src.y + src.h, dst.y, dst.y + dst.h);
                if ov > 0 { Some((dst.x - (src.x + src.w), ov)) } else { None }
            }
            Dir::Left if dst.x + dst.w <= src.x => {
                let ov = overlap_1d(src.y, src.y + src.h, dst.y, dst.y + dst.h);
                if ov > 0 { Some((src.x - (dst.x + dst.w), ov)) } else { None }
            }
            Dir::Down if dst.y >= src.y + src.h => {
                let ov = overlap_1d(src.x, src.x + src.w, dst.x, dst.x + dst.w);
                if ov > 0 { Some((dst.y - (src.y + src.h), ov)) } else { None }
            }
            Dir::Up if dst.y + dst.h <= src.y => {
                let ov = overlap_1d(src.x, src.x + src.w, dst.x, dst.x + dst.w);
                if ov > 0 { Some((src.y - (dst.y + dst.h), ov)) } else { None }
            }
            _ => None,
        };

        if let Some((gap, ov)) = candidate {
            let better = match best {
                None => true,
                Some((bgap, bov, _)) => gap < bgap || (gap == bgap && ov > bov),
            };
            if better {
                best = Some((gap, ov, id));
            }
        }
    }
    best.map(|(_, _, id)| id)
}

fn overlap_1d(a0: usize, a1: usize, b0: usize, b1: usize) -> usize {
    let s = a0.max(b0);
    let e = a1.min(b1);
    e.saturating_sub(s)
}

fn compute_junctions(out: &mut ComputedLayout) {
    if out.dividers.len() < 2 {
        return;
    }
    let mut v_cells: HashSet<(usize, usize)> = HashSet::new();
    let mut h_cells: HashSet<(usize, usize)> = HashSet::new();

    for d in &out.dividers {
        match d.axis {
            SplitAxis::Horizontal => {
                for r in d.y..(d.y + d.len) {
                    v_cells.insert((d.x, r));
                }
            }
            SplitAxis::Vertical => {
                for c in d.x..(d.x + d.len) {
                    h_cells.insert((c, d.y));
                }
            }
        }
    }

    let mut jmap: HashMap<(usize, usize), char> = HashMap::new();

    for &(cx, cy) in &v_cells {
        let l = cx > 0 && h_cells.contains(&(cx - 1, cy));
        let r = h_cells.contains(&(cx + 1, cy));
        if l || r {
            let u = cy > 0 && v_cells.contains(&(cx, cy - 1));
            let d = v_cells.contains(&(cx, cy + 1));
            if let Some(ch) = box_junction(u, d, l, r) {
                jmap.insert((cx, cy), ch);
            }
        }
    }

    for &(cx, cy) in &h_cells {
        let u = cy > 0 && v_cells.contains(&(cx, cy - 1));
        let d = v_cells.contains(&(cx, cy + 1));
        if u || d {
            let l = cx > 0 && h_cells.contains(&(cx - 1, cy));
            let r = h_cells.contains(&(cx + 1, cy));
            if let Some(ch) = box_junction(u, d, l, r) {
                jmap.insert((cx, cy), ch);
            }
        }
    }

    out.junctions = jmap.into_iter().map(|((x, y), ch)| (x, y, ch)).collect();
}

fn box_junction(u: bool, d: bool, l: bool, r: bool) -> Option<char> {
    match (u, d, l, r) {
        (true, true, true, true) => Some('┼'),
        (true, true, false, true) => Some('├'),
        (true, true, true, false) => Some('┤'),
        (false, true, true, true) => Some('┬'),
        (true, false, true, true) => Some('┴'),
        (false, true, false, true) => Some('┌'),
        (false, true, true, false) => Some('┐'),
        (true, false, false, true) => Some('└'),
        (true, false, true, false) => Some('┘'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_splits_compute_exact_rects_and_junctions() {
        let mut root = LayoutNode::leaf(0);
        assert!(root.split_pane(0, 1, 10, SplitAxis::Horizontal, false));
        assert!(root.split_pane(1, 2, 11, SplitAxis::Vertical, false));
        assert_eq!(root.pane_count(), 3);
        assert_eq!(root.pane_order(), vec![0, 1, 2]);

        let area = Rect { x: 0, y: 0, w: 101, h: 31 };
        let comp = root.compute(area);
        assert_eq!(comp.panes.len(), 3);
        assert_eq!(comp.dividers.len(), 2);

        let r0 = comp.rect_of(0).unwrap();
        let r1 = comp.rect_of(1).unwrap();
        let r2 = comp.rect_of(2).unwrap();
        assert_eq!(r0, Rect { x: 0, y: 0, w: 50, h: 31 });
        assert_eq!(r1, Rect { x: 51, y: 0, w: 50, h: 15 });
        assert_eq!(r2, Rect { x: 51, y: 16, w: 50, h: 15 });

        // T-junction `├` where horizontal divider (y=15, x=51..101) meets vertical divider (x=50)
        assert!(comp.junctions.contains(&(50, 15, '├')));

        // Directional neighbor queries
        assert_eq!(find_neighbor(0, Dir::Right, &comp), Some(1));
        assert_eq!(find_neighbor(1, Dir::Left, &comp), Some(0));
        assert_eq!(find_neighbor(1, Dir::Down, &comp), Some(2));
        assert_eq!(find_neighbor(2, Dir::Up, &comp), Some(1));
        assert_eq!(find_neighbor(2, Dir::Left, &comp), Some(0));
        assert_eq!(find_neighbor(0, Dir::Left, &comp), None);
    }

    #[test]
    fn resize_toggle_swap_and_remove() {
        let mut root = LayoutNode::leaf(0);
        root.split_pane(0, 1, 1, SplitAxis::Horizontal, false);

        // Grow pane 1 (second child) -> decreases first child's ratio
        assert!(root.resize_pane_grow(1, 0.10));
        let comp = root.compute(Rect { x: 0, y: 0, w: 101, h: 20 });
        assert_eq!(comp.rect_of(0).unwrap().w, 40);
        assert_eq!(comp.rect_of(1).unwrap().w, 60);

        // Equalize back to 50/50
        root.equalize_all();
        let comp = root.compute(Rect { x: 0, y: 0, w: 101, h: 20 });
        assert_eq!(comp.rect_of(0).unwrap().w, 50);
        assert_eq!(comp.rect_of(1).unwrap().w, 50);

        // Toggle orientation Horizontal -> Vertical
        assert_eq!(root.toggle_axis(0), Some(SplitAxis::Vertical));
        let comp = root.compute(Rect { x: 0, y: 0, w: 101, h: 21 });
        assert_eq!(comp.rect_of(0).unwrap(), Rect { x: 0, y: 0, w: 101, h: 10 });
        assert_eq!(comp.rect_of(1).unwrap(), Rect { x: 0, y: 11, w: 101, h: 10 });

        // Swap siblings
        assert!(root.swap_sibling(0));
        assert_eq!(root.pane_order(), vec![1, 0]);

        // Remove pane 1 -> collapses to Leaf(0)
        assert_eq!(root.remove_pane(1), Some(0));
        assert_eq!(root.pane_count(), 1);
        assert_eq!(root.pane_order(), vec![0]);
    }

    #[test]
    fn shared_doc_view_clamps_utf8_and_scroll_safely() {
        let mut doc = crate::document::Document::new_empty();
        doc.paste("ação é ótimo\nlinha 2");
        let mut cursors = vec![crate::document::Selection {
            anchor: 999,
            head: 2, // middle of multi-byte UTF-8 'ç'
            desired_col: 0,
        }];
        let mut primary = 5;
        let mut scroll_row = 40;
        doc.clamp_view_state(&mut cursors, &mut primary, &mut scroll_row);
        assert_eq!(primary, 0);
        assert_eq!(scroll_row, 1);
        assert!(cursors[0].anchor <= doc.rope.len_bytes());
        assert!(doc.rope.slice(0, cursors[0].head).is_ascii() || cursors[0].head <= doc.rope.len_bytes());
    }
}
