#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Enter,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    F(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyEvent {
    pub fn plain(code: KeyCode) -> Self {
        Self {
            code,
            ctrl: false,
            alt: false,
            shift: false,
        }
    }

    pub fn ctrl(code: KeyCode) -> Self {
        Self {
            code,
            ctrl: true,
            alt: false,
            shift: false,
        }
    }

    pub fn alt(code: KeyCode) -> Self {
        Self {
            code,
            ctrl: false,
            alt: true,
            shift: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseKind {
    LeftDown,
    LeftDrag,
    LeftUp,
    WheelUp,
    WheelDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseEvent {
    pub kind: MouseKind,
    pub col: usize,
    pub row: usize,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Key(KeyEvent),
    Paste(String),
    Mouse(MouseEvent),
}

pub struct InputDecoder {
    buf: Vec<u8>,
    in_paste: bool,
    paste_buf: Vec<u8>,
}

const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

impl InputDecoder {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(256),
            in_paste: false,
            paste_buf: Vec::new(),
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    pub fn has_pending_esc(&self) -> bool {
        !self.in_paste && self.buf.len() == 1 && self.buf[0] == 0x1b
    }

    /// Call after a short timeout (~25ms) if `has_pending_esc()` is true.
    pub fn flush_bare_esc(&mut self) -> Option<Event> {
        if self.has_pending_esc() {
            self.buf.clear();
            Some(Event::Key(KeyEvent::plain(KeyCode::Esc)))
        } else {
            None
        }
    }

    pub fn next_event(&mut self) -> Option<Event> {
        loop {
            if self.in_paste {
                if let Some(pos) = find_subslice(&self.buf, PASTE_END) {
                    self.paste_buf.extend_from_slice(&self.buf[..pos]);
                    self.buf.drain(..pos + PASTE_END.len());
                    self.in_paste = false;
                    let raw = String::from_utf8_lossy(&self.paste_buf).into_owned();
                    self.paste_buf.clear();
                    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
                    return Some(Event::Paste(normalized));
                } else {
                    // Keep up to PASTE_END.len() - 1 trailing bytes in case marker spans reads
                    if self.buf.len() >= PASTE_END.len() {
                        let take = self.buf.len() - (PASTE_END.len() - 1);
                        self.paste_buf.extend_from_slice(&self.buf[..take]);
                        self.buf.drain(..take);
                    }
                    return None;
                }
            }

            if self.buf.is_empty() {
                return None;
            }

            // Bracketed paste start?
            if self.buf.starts_with(PASTE_START) {
                self.buf.drain(..PASTE_START.len());
                self.in_paste = true;
                self.paste_buf.clear();
                continue;
            }
            if PASTE_START.starts_with(&self.buf) && self.buf.len() > 1 {
                return None; // Wait for remaining bytes of PASTE_START
            }

            let b = self.buf[0];

            // Escape sequence
            if b == 0x1b {
                if self.buf.len() == 1 {
                    return None; // Let caller poll 25ms then call flush_bare_esc()
                }
                let b1 = self.buf[1];
                if b1 == b'[' {
                    return self.parse_csi();
                } else if b1 == b'O' {
                    if self.buf.len() < 3 {
                        return None;
                    }
                    let b2 = self.buf[2];
                    self.buf.drain(..3);
                    let code = match b2 {
                        b'P' => KeyCode::F(1),
                        b'Q' => KeyCode::F(2),
                        b'R' => KeyCode::F(3),
                        b'S' => KeyCode::F(4),
                        b'A' => KeyCode::Up,
                        b'B' => KeyCode::Down,
                        b'C' => KeyCode::Right,
                        b'D' => KeyCode::Left,
                        b'H' => KeyCode::Home,
                        b'F' => KeyCode::End,
                        _ => continue,
                    };
                    return Some(Event::Key(KeyEvent::plain(code)));
                } else if b1 == b'\r' || b1 == b'\n' {
                    // Shift+Enter / Alt+Enter (`\x1b\r` from WT)
                    self.buf.drain(..2);
                    return Some(Event::Key(KeyEvent {
                        code: KeyCode::Enter,
                        ctrl: false,
                        alt: false,
                        shift: true,
                    }));
                } else if b1 == 0x7f || b1 == 0x08 {
                    self.buf.drain(..2);
                    return Some(Event::Key(KeyEvent::alt(KeyCode::Backspace)));
                } else if b1 == 0x1b {
                    self.buf.drain(..1);
                    return Some(Event::Key(KeyEvent::plain(KeyCode::Esc)));
                } else {
                    // Alt + UTF-8 char
                    if let Some((ch, n)) = decode_utf8_char(&self.buf[1..]) {
                        self.buf.drain(..1 + n);
                        return Some(Event::Key(KeyEvent {
                            code: KeyCode::Char(ch.to_ascii_lowercase()),
                            ctrl: false,
                            alt: true,
                            shift: ch.is_ascii_uppercase(),
                        }));
                    }
                    return None;
                }
            }

            // Control bytes & ASCII specials
            if b < 0x20 || b == 0x7f {
                self.buf.drain(..1);
                let ev = match b {
                    0x00 => KeyEvent::ctrl(KeyCode::Char(' ')),
                    0x08 => KeyEvent::ctrl(KeyCode::Backspace), // WT Ctrl+Backspace
                    0x09 => KeyEvent::plain(KeyCode::Tab),
                    0x0a | 0x0d => KeyEvent::plain(KeyCode::Enter),
                    0x1f => KeyEvent::ctrl(KeyCode::Char('/')), // Ctrl+/
                    0x7f => KeyEvent::plain(KeyCode::Backspace),
                    1..=26 => KeyEvent::ctrl(KeyCode::Char((b'a' + (b - 1)) as char)),
                    _ => continue,
                };
                return Some(Event::Key(ev));
            }

            // Regular UTF-8 char
            if let Some((ch, n)) = decode_utf8_char(&self.buf) {
                self.buf.drain(..n);
                return Some(Event::Key(KeyEvent::plain(KeyCode::Char(ch))));
            }
            return None;
        }
    }

    fn parse_csi(&mut self) -> Option<Event> {
        // Need at least ESC [ <final>
        let mut i = 2;
        while i < self.buf.len() {
            let c = self.buf[i];
            if (0x40..=0x7e).contains(&c) {
                let params = String::from_utf8_lossy(&self.buf[2..i]).into_owned();
                let final_byte = c as char;
                self.buf.drain(..=i);
                return decode_csi(&params, final_byte);
            }
            i += 1;
            if i > 64 {
                self.buf.drain(..2);
                return None;
            }
        }
        None
    }
}

fn decode_modifiers(m: u32) -> (bool, bool, bool) {
    let bits = m.saturating_sub(1);
    let shift = (bits & 1) != 0;
    let alt = (bits & 2) != 0;
    let ctrl = (bits & 4) != 0;
    (ctrl, alt, shift)
}

fn decode_csi(params: &str, final_ch: char) -> Option<Event> {
    // SGR 1006 Mouse: ESC [ < btn ; col ; row M/m
    if let Some(rest) = params.strip_prefix('<') {
        if final_ch == 'M' || final_ch == 'm' {
            let nums: Vec<u32> = rest.split(';').filter_map(|p| p.parse().ok()).collect();
            if nums.len() == 3 {
                let btn = nums[0];
                let col = nums[1].saturating_sub(1) as usize;
                let row = nums[2].saturating_sub(1) as usize;
                let shift = (btn & 4) != 0;
                let alt = (btn & 8) != 0;
                let ctrl = (btn & 16) != 0;
                let motion = (btn & 32) != 0;
                let wheel = (btn & 64) != 0;
                let base = btn & 3;

                let kind = if wheel {
                    if base == 0 {
                        MouseKind::WheelUp
                    } else if base == 1 {
                        MouseKind::WheelDown
                    } else {
                        return None;
                    }
                } else if final_ch == 'm' {
                    if base == 0 {
                        MouseKind::LeftUp
                    } else {
                        return None;
                    }
                } else if motion && base == 0 {
                    MouseKind::LeftDrag
                } else if !motion && base == 0 {
                    MouseKind::LeftDown
                } else {
                    return None;
                };

                return Some(Event::Mouse(MouseEvent {
                    kind,
                    col,
                    row,
                    ctrl,
                    alt,
                    shift,
                }));
            }
        }
        return None;
    }

    let parts: Vec<u32> = if params.is_empty() {
        vec![]
    } else {
        params.split(';').filter_map(|p| p.parse().ok()).collect()
    };

    // CSI u (Kitty / Windows Terminal sendInput): ESC [ <codepoint> ; <mod> u
    if final_ch == 'u' {
        let cp = *parts.first()?;
        let (ctrl, alt, shift) = parts.get(1).copied().map(decode_modifiers).unwrap_or((false, false, false));
        let code = match cp {
            127 | 8 => KeyCode::Backspace,
            13 => KeyCode::Enter,
            9 => KeyCode::Tab,
            27 => KeyCode::Esc,
            _ => {
                let ch = char::from_u32(cp)?;
                KeyCode::Char(ch.to_ascii_lowercase())
            }
        };
        return Some(Event::Key(KeyEvent { code, ctrl, alt, shift }));
    }

    let (ctrl, alt, shift) = if parts.len() >= 2 {
        decode_modifiers(parts[1])
    } else {
        (false, false, false)
    };

    let code = match final_ch {
        'A' => KeyCode::Up,
        'B' => KeyCode::Down,
        'C' => KeyCode::Right,
        'D' => KeyCode::Left,
        'H' => KeyCode::Home,
        'F' => KeyCode::End,
        'Z' => return Some(Event::Key(KeyEvent {
            code: KeyCode::BackTab,
            ctrl: false,
            alt: false,
            shift: true,
        })),
        'P' => KeyCode::F(1),
        'Q' => KeyCode::F(2),
        'R' => KeyCode::F(3),
        'S' => KeyCode::F(4),
        '~' => match parts.first().copied().unwrap_or(0) {
            1 | 7 => KeyCode::Home,
            2 => KeyCode::Insert,
            3 => KeyCode::Delete,
            4 | 8 => KeyCode::End,
            5 => KeyCode::PageUp,
            6 => KeyCode::PageDown,
            15 => KeyCode::F(5),
            17 => KeyCode::F(6),
            18 => KeyCode::F(7),
            19 => KeyCode::F(8),
            _ => return None,
        },
        _ => return None,
    };

    Some(Event::Key(KeyEvent {
        code,
        ctrl,
        alt,
        shift,
    }))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn decode_utf8_char(bytes: &[u8]) -> Option<(char, usize)> {
    if bytes.is_empty() {
        return None;
    }
    let first = bytes[0];
    let needed = if first < 0x80 {
        1
    } else if (first & 0xE0) == 0xC0 {
        2
    } else if (first & 0xF0) == 0xE0 {
        3
    } else if (first & 0xF8) == 0xF0 {
        4
    } else {
        return Some(('\u{FFFD}', 1));
    };
    if bytes.len() < needed {
        return None;
    }
    match std::str::from_utf8(&bytes[..needed]) {
        Ok(s) => s.chars().next().map(|c| (c, needed)),
        Err(_) => Some(('\u{FFFD}', 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utf8_csiu_paste_and_mouse() {
        let mut d = InputDecoder::new();
        d.push("çã".as_bytes());
        assert_eq!(d.next_event(), Some(Event::Key(KeyEvent::plain(KeyCode::Char('ç')))));
        assert_eq!(d.next_event(), Some(Event::Key(KeyEvent::plain(KeyCode::Char('ã')))));

        // CSI u: Ctrl+Shift+Z, Ctrl+Shift+L, Ctrl+; (47;5u)
        d.push(b"\x1b[90;6u\x1b[76;6u\x1b[47;5u");
        assert_eq!(
            d.next_event(),
            Some(Event::Key(KeyEvent { code: KeyCode::Char('z'), ctrl: true, alt: false, shift: true }))
        );
        assert_eq!(
            d.next_event(),
            Some(Event::Key(KeyEvent { code: KeyCode::Char('l'), ctrl: true, alt: false, shift: true }))
        );
        assert_eq!(
            d.next_event(),
            Some(Event::Key(KeyEvent { code: KeyCode::Char('/'), ctrl: true, alt: false, shift: false }))
        );

        // Bracketed paste
        d.push(b"\x1b[200~hello\r\nworld\x1b[201~");
        assert_eq!(d.next_event(), Some(Event::Paste("hello\nworld".into())));

        // SGR Mouse left click at col=10, row=5 (1-based 11, 6)
        d.push(b"\x1b[<0;11;6M");
        assert_eq!(
            d.next_event(),
            Some(Event::Mouse(MouseEvent {
                kind: MouseKind::LeftDown,
                col: 10,
                row: 5,
                ctrl: false,
                alt: false,
                shift: false,
            }))
        );
    }
}
