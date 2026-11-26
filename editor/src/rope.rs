use std::cmp::max;

const MAX_LEAF: usize = 2048;
const MIN_LEAF: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn label(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::Crlf => "CRLF",
        }
    }
}

#[derive(Clone, Debug)]
enum Node {
    Leaf {
        text: String,
        lines: usize,
    },
    Branch {
        left: Box<Node>,
        right: Box<Node>,
        left_bytes: usize,
        left_lines: usize,
        total_bytes: usize,
        total_lines: usize,
        height: u8,
    },
}

fn count_newlines(s: &str) -> usize {
    s.as_bytes().iter().filter(|&&b| b == b'\n').count()
}

fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

impl Node {
    fn leaf(text: String) -> Box<Self> {
        let lines = count_newlines(&text);
        Box::new(Node::Leaf { text, lines })
    }

    fn from_str(s: &str) -> Box<Self> {
        if s.len() <= MAX_LEAF {
            return Self::leaf(s.to_string());
        }
        let mid = floor_char_boundary(s, s.len() / 2);
        if mid == 0 || mid == s.len() {
            return Self::leaf(s.to_string());
        }
        let left = Self::from_str(&s[..mid]);
        let right = Self::from_str(&s[mid..]);
        Self::branch(left, right)
    }

    fn len_bytes(&self) -> usize {
        match self {
            Node::Leaf { text, .. } => text.len(),
            Node::Branch { total_bytes, .. } => *total_bytes,
        }
    }

    fn newlines(&self) -> usize {
        match self {
            Node::Leaf { lines, .. } => *lines,
            Node::Branch { total_lines, .. } => *total_lines,
        }
    }

    fn height(&self) -> u8 {
        match self {
            Node::Leaf { .. } => 1,
            Node::Branch { height, .. } => *height,
        }
    }

    fn branch(left: Box<Self>, right: Box<Self>) -> Box<Self> {
        let left_bytes = left.len_bytes();
        let left_lines = left.newlines();
        let total_bytes = left_bytes + right.len_bytes();
        let total_lines = left_lines + right.newlines();
        let height = 1 + max(left.height(), right.height());
        Box::new(Node::Branch {
            left,
            right,
            left_bytes,
            left_lines,
            total_bytes,
            total_lines,
            height,
        })
    }

    fn rotate_right(self: Box<Self>) -> Box<Self> {
        match *self {
            Node::Branch { left, right, .. } => match *left {
                Node::Branch {
                    left: ll,
                    right: lr,
                    ..
                } => Self::branch(ll, Self::branch(lr, right)),
                leaf => Self::branch(Box::new(leaf), right),
            },
            leaf => Box::new(leaf),
        }
    }

    fn rotate_left(self: Box<Self>) -> Box<Self> {
        match *self {
            Node::Branch { left, right, .. } => match *right {
                Node::Branch {
                    left: rl,
                    right: rr,
                    ..
                } => Self::branch(Self::branch(left, rl), rr),
                leaf => Self::branch(left, Box::new(leaf)),
            },
            leaf => Box::new(leaf),
        }
    }

    fn balance(self: Box<Self>) -> Box<Self> {
        match *self {
            Node::Leaf { .. } => self,
            Node::Branch { left, right, .. } => {
                let lh = left.height() as i16;
                let rh = right.height() as i16;
                if lh > rh + 1 {
                    let left = match *left {
                        Node::Branch {
                            left: ll,
                            right: lr,
                            ..
                        } if lr.height() > ll.height() => Self::branch(ll, lr).rotate_left(),
                        other => Box::new(other),
                    };
                    Self::branch(left, right).rotate_right()
                } else if rh > lh + 1 {
                    let right = match *right {
                        Node::Branch {
                            left: rl,
                            right: rr,
                            ..
                        } if rl.height() > rr.height() => Self::branch(rl, rr).rotate_right(),
                        other => Box::new(other),
                    };
                    Self::branch(left, right).rotate_left()
                } else {
                    Self::branch(left, right)
                }
            }
        }
    }

    fn concat(left: Box<Self>, right: Box<Self>) -> Box<Self> {
        if left.len_bytes() == 0 {
            return right;
        }
        if right.len_bytes() == 0 {
            return left;
        }
        // Coalesce small adjacent leaves to keep tree compact
        if let (
            Node::Leaf { text: lt, .. },
            Node::Leaf { text: rt, .. },
        ) = (left.as_ref(), right.as_ref())
        {
            if lt.len() + rt.len() <= MIN_LEAF {
                let mut combined = String::with_capacity(lt.len() + rt.len());
                combined.push_str(lt);
                combined.push_str(rt);
                return Self::leaf(combined);
            }
        }
        let lh = left.height() as i16;
        let rh = right.height() as i16;
        if lh > rh + 1 {
            if let Node::Branch {
                left: ll,
                right: lr,
                ..
            } = *left
            {
                let new_right = Self::concat(lr, right);
                return Self::branch(ll, new_right).balance();
            }
        } else if rh > lh + 1 {
            if let Node::Branch {
                left: rl,
                right: rr,
                ..
            } = *right
            {
                let new_left = Self::concat(left, rl);
                return Self::branch(new_left, rr).balance();
            }
        }
        Self::branch(left, right).balance()
    }

    fn split(self: Box<Self>, byte_idx: usize) -> (Box<Self>, Box<Self>) {
        if byte_idx == 0 {
            return (Self::leaf(String::new()), self);
        }
        if byte_idx >= self.len_bytes() {
            return (self, Self::leaf(String::new()));
        }
        match *self {
            Node::Leaf { text, .. } => {
                let cut = floor_char_boundary(&text, byte_idx);
                let l = text[..cut].to_string();
                let r = text[cut..].to_string();
                (Self::leaf(l), Self::leaf(r))
            }
            Node::Branch {
                left,
                right,
                left_bytes,
                ..
            } => {
                if byte_idx == left_bytes {
                    (left, right)
                } else if byte_idx < left_bytes {
                    let (ll, lr) = left.split(byte_idx);
                    (ll, Self::concat(lr, right))
                } else {
                    let (rl, rr) = right.split(byte_idx - left_bytes);
                    (Self::concat(left, rl), rr)
                }
            }
        }
    }

    fn line_start_byte(&self, target_line: usize) -> usize {
        if target_line == 0 {
            return 0;
        }
        match self {
            Node::Leaf { text, .. } => {
                let mut seen = 0usize;
                for (i, &b) in text.as_bytes().iter().enumerate() {
                    if b == b'\n' {
                        seen += 1;
                        if seen == target_line {
                            return i + 1;
                        }
                    }
                }
                text.len()
            }
            Node::Branch {
                left,
                right,
                left_bytes,
                left_lines,
                ..
            } => {
                if target_line <= *left_lines {
                    left.line_start_byte(target_line)
                } else {
                    left_bytes + right.line_start_byte(target_line - left_lines)
                }
            }
        }
    }

    fn byte_to_line(&self, byte_idx: usize) -> usize {
        match self {
            Node::Leaf { text, .. } => {
                let end = byte_idx.min(text.len());
                count_newlines(&text[..floor_char_boundary(text, end)])
            }
            Node::Branch {
                left,
                right,
                left_bytes,
                left_lines,
                ..
            } => {
                if byte_idx <= *left_bytes {
                    left.byte_to_line(byte_idx)
                } else {
                    left_lines + right.byte_to_line(byte_idx - left_bytes)
                }
            }
        }
    }

    fn collect_range(&self, start: usize, end: usize, out: &mut String) {
        if start >= end || start >= self.len_bytes() {
            return;
        }
        let end = end.min(self.len_bytes());
        match self {
            Node::Leaf { text, .. } => {
                let s = floor_char_boundary(text, start);
                let e = floor_char_boundary(text, end);
                if s < e {
                    out.push_str(&text[s..e]);
                }
            }
            Node::Branch {
                left,
                right,
                left_bytes,
                ..
            } => {
                if start < *left_bytes {
                    left.collect_range(start, end.min(*left_bytes), out);
                }
                if end > *left_bytes {
                    right.collect_range(start.saturating_sub(*left_bytes), end - *left_bytes, out);
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Rope {
    root: Box<Node>,
}

impl Default for Rope {
    fn default() -> Self {
        Self::new()
    }
}

impl Rope {
    pub fn new() -> Self {
        Self {
            root: Node::leaf(String::new()),
        }
    }

    pub fn from_str_detect_eol(raw: &str) -> (Self, LineEnding) {
        let eol = if raw.contains("\r\n") {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        };
        if eol == LineEnding::Crlf {
            let normalized = raw.replace("\r\n", "\n");
            (Self::from_str(&normalized), eol)
        } else {
            (Self::from_str(raw), eol)
        }
    }

    pub fn from_str(s: &str) -> Self {
        Self {
            root: Node::from_str(s),
        }
    }

    pub fn len_bytes(&self) -> usize {
        self.root.len_bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.len_bytes() == 0
    }

    pub fn line_count(&self) -> usize {
        self.root.newlines() + 1
    }

    pub fn line_start_byte(&self, line: usize) -> usize {
        if line >= self.line_count() {
            self.len_bytes()
        } else {
            self.root.line_start_byte(line)
        }
    }

    /// End byte index of `line` EXCLUDING its trailing `\n` (if any).
    pub fn line_end_byte(&self, line: usize) -> usize {
        if line + 1 >= self.line_count() {
            self.len_bytes()
        } else {
            self.root.line_start_byte(line + 1).saturating_sub(1)
        }
    }

    pub fn line(&self, line: usize) -> String {
        if line >= self.line_count() {
            return String::new();
        }
        let s = self.line_start_byte(line);
        let e = self.line_end_byte(line);
        self.slice(s, e)
    }

    pub fn slice(&self, start: usize, end: usize) -> String {
        let s = start.min(self.len_bytes());
        let e = end.min(self.len_bytes());
        if s >= e {
            return String::new();
        }
        let mut out = String::with_capacity(e - s);
        self.root.collect_range(s, e, &mut out);
        out
    }

    pub fn to_string_with_eol(&self, eol: LineEnding) -> String {
        let text = self.slice(0, self.len_bytes());
        match eol {
            LineEnding::Lf => text,
            LineEnding::Crlf => text.replace('\n', "\r\n"),
        }
    }

    pub fn byte_to_line_col(&self, byte_idx: usize) -> (usize, usize) {
        let clamped = byte_idx.min(self.len_bytes());
        let line = self.root.byte_to_line(clamped);
        let start = self.line_start_byte(line);
        let col = self.slice(start, clamped).chars().count();
        (line, col)
    }

    pub fn line_col_to_byte(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.line_count().saturating_sub(1));
        let start = self.line_start_byte(line);
        let text = self.line(line);
        let mut offset = 0usize;
        for (i, ch) in text.chars().enumerate() {
            if i == col {
                return start + offset;
            }
            offset += ch.len_utf8();
        }
        start + text.len()
    }

    pub fn insert(&mut self, byte_idx: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let root = std::mem::replace(&mut self.root, Node::leaf(String::new()));
        let (left, right) = root.split(byte_idx);
        let mid = Node::from_str(text);
        self.root = Node::concat(Node::concat(left, mid), right);
    }

    pub fn remove(&mut self, start: usize, end: usize) -> String {
        let s = start.min(self.len_bytes());
        let e = end.min(self.len_bytes());
        if s >= e {
            return String::new();
        }
        let removed = self.slice(s, e);
        let root = std::mem::replace(&mut self.root, Node::leaf(String::new()));
        let (left, rest) = root.split(s);
        let (_, right) = rest.split(e - s);
        self.root = Node::concat(left, right);
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_insert_remove_utf8() {
        let mut r = Rope::from_str("olá\nmundo 🚀\nação");
        assert_eq!(r.line_count(), 3);
        assert_eq!(r.line(0), "olá");
        assert_eq!(r.line(1), "mundo 🚀");
        assert_eq!(r.line(2), "ação");

        let b = r.line_col_to_byte(1, 6);
        r.insert(b, "nave ");
        assert_eq!(r.line(1), "mundo nave 🚀");
        assert_eq!(r.byte_to_line_col(r.line_col_to_byte(2, 2)), (2, 2));

        let s = r.line_col_to_byte(1, 6);
        let e = r.line_col_to_byte(1, 11);
        assert_eq!(r.remove(s, e), "nave ");
        assert_eq!(r.line(1), "mundo 🚀");
    }

    #[test]
    fn large_50k_lines_stress_and_balance() {
        let mut big = String::new();
        for i in 0..50_000 {
            big.push_str(&format!("linha {:05}: conteúdo UTF-8 ação\n", i));
        }
        let mut r = Rope::from_str(&big);
        assert_eq!(r.line_count(), 50_001);
        assert_eq!(r.line(12_345), "linha 12345: conteúdo UTF-8 ação");
        assert!(r.root.height() <= 24, "rope unbalanced: h={}", r.root.height());

        let pos = r.line_col_to_byte(25_000, 13);
        r.insert(pos, "EDITADO ");
        assert_eq!(r.line(25_000), "linha 25000: EDITADO conteúdo UTF-8 ação");
    }

    #[test]
    fn crlf_roundtrip() {
        let (r, eol) = Rope::from_str_detect_eol("a\r\nb\r\nc");
        assert_eq!(eol, LineEnding::Crlf);
        assert_eq!(r.line_count(), 3);
        assert_eq!(r.to_string_with_eol(eol), "a\r\nb\r\nc");
    }
}
