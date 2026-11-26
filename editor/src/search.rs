use crate::rope::Rope;
use regex_lite::RegexBuilder;

#[derive(Clone, Debug, Default)]
pub struct SearchState {
    pub query: String,
    pub replace: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
    pub matches: Vec<(usize, usize)>,
    pub active_idx: Option<usize>,
    pub error: Option<String>,
}

impl SearchState {
    pub fn recompute(&mut self, rope: &Rope, cursor_byte: usize) {
        self.matches.clear();
        self.active_idx = None;
        self.error = None;

        if self.query.is_empty() {
            return;
        }

        let text = rope.slice(0, rope.len_bytes());
        let mut pattern = if self.use_regex {
            self.query.clone()
        } else {
            regex_lite::escape(&self.query)
        };
        if self.whole_word {
            pattern = format!(r"\b(?:{})\b", pattern);
        }

        let re = match RegexBuilder::new(&pattern)
            .case_insensitive(!self.case_sensitive)
            .multi_line(true)
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(format!("Regex invalida: {}", e));
                return;
            }
        };

        for m in re.find_iter(&text) {
            if m.start() < m.end() {
                self.matches.push((m.start(), m.end()));
                if self.matches.len() >= 20_000 {
                    break;
                }
            }
        }

        if !self.matches.is_empty() {
            let idx = self
                .matches
                .iter()
                .position(|&(s, _)| s >= cursor_byte)
                .unwrap_or(0);
            self.active_idx = Some(idx);
        }
    }

    pub fn next_match(&mut self) -> Option<(usize, usize)> {
        if self.matches.is_empty() {
            return None;
        }
        let next = match self.active_idx {
            Some(i) => (i + 1) % self.matches.len(),
            None => 0,
        };
        self.active_idx = Some(next);
        Some(self.matches[next])
    }

    pub fn prev_match(&mut self) -> Option<(usize, usize)> {
        if self.matches.is_empty() {
            return None;
        }
        let prev = match self.active_idx {
            Some(0) | None => self.matches.len() - 1,
            Some(i) => i - 1,
        };
        self.active_idx = Some(prev);
        Some(self.matches[prev])
    }

    pub fn active_match(&self) -> Option<(usize, usize)> {
        self.active_idx.and_then(|i| self.matches.get(i).copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_whole_word_and_regex_search() {
        let r = Rope::from_str("foo bar Foo foobar foo123");
        let mut s = SearchState::default();
        s.query = "foo".into();
        s.recompute(&r, 0);
        assert_eq!(s.matches.len(), 4);

        s.case_sensitive = true;
        s.recompute(&r, 0);
        assert_eq!(s.matches.len(), 3);

        s.whole_word = true;
        s.recompute(&r, 0);
        assert_eq!(s.matches.len(), 1);

        s.whole_word = false;
        s.use_regex = true;
        s.query = r"foo\d+".into();
        s.recompute(&r, 0);
        assert_eq!(s.matches.len(), 1);
        assert_eq!(r.slice(s.matches[0].0, s.matches[0].1), "foo123");
    }
}
