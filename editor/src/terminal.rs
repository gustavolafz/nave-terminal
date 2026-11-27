use std::io::{self, Write};
use std::os::unix::io::RawFd;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static WINCH_FLAG: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_winch(_sig: libc::c_int) {
    WINCH_FLAG.store(true, Ordering::SeqCst);
}

pub struct Terminal {
    orig: libc::termios,
    fd: RawFd,
    active: bool,
}

impl Terminal {
    pub fn enter(block_cursor: bool, cursor_hex: &str) -> io::Result<Self> {
        let fd = libc::STDIN_FILENO;
        let mut orig: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut orig) } != 0 {
            return Err(io::Error::last_os_error());
        }

        let mut raw = orig;
        raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        raw.c_oflag &= !(libc::OPOST);
        raw.c_cflag |= libc::CS8;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 0;

        if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }

        // Install SIGWINCH handler for instant resize redraws
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = handle_winch as *const () as usize;
            sa.sa_flags = 0;
            libc::sigemptyset(&mut sa.sa_mask);
            libc::sigaction(libc::SIGWINCH, &sa, std::ptr::null_mut());
        }

        let mut stdout = io::stdout().lock();
        // Always enable SGR 1006 button-drag mouse reporting so pane divider resizing & tab drag-and-drop always work
        stdout.write_all(b"\x1b[?1049h\x1b[?2004h\x1b[?1000h\x1b[?1002h\x1b[?1006h")?;
        stdout.write_all(if block_cursor { b"\x1b[2 q" } else { b"\x1b[6 q" })?;
        if !cursor_hex.is_empty() {
            let _ = write!(stdout, "\x1b]12;{}\x07", cursor_hex);
        }
        stdout.write_all(b"\x1b[2J\x1b[H")?;
        stdout.flush()?;

        Ok(Self {
            orig,
            fd,
            active: true,
        })
    }

    pub fn set_cursor_color(&self, cursor_hex: &str) {
        if cursor_hex.is_empty() {
            return;
        }
        let mut stdout = io::stdout().lock();
        let _ = write!(stdout, "\x1b]12;{}\x07", cursor_hex);
        let _ = stdout.flush();
    }

    pub fn set_cursor_style(&self, block: bool, blinking: bool) {
        let seq: &[u8] = match (block, blinking) {
            (true, true) => b"\x1b[1 q",
            (true, false) => b"\x1b[2 q",
            (false, true) => b"\x1b[5 q",
            (false, false) => b"\x1b[6 q",
        };
        let mut stdout = io::stdout().lock();
        let _ = stdout.write_all(seq);
        let _ = stdout.flush();
    }

    pub fn size() -> (usize, usize) {
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) };
        if rc == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
            (ws.ws_col as usize, ws.ws_row as usize)
        } else {
            (100, 30)
        }
    }

    pub fn take_resized() -> bool {
        WINCH_FLAG.swap(false, Ordering::SeqCst)
    }

    pub fn poll_stdin(&self, timeout_ms: i32) -> bool {
        let mut pfd = libc::pollfd {
            fd: self.fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let rc = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        rc > 0 && (pfd.revents & libc::POLLIN) != 0
    }

    pub fn read_bytes(&self, buf: &mut [u8]) -> usize {
        let n = unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n > 0 {
            n as usize
        } else {
            0
        }
    }

    pub fn write_frame(&self, data: &str) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        stdout.write_all(data.as_bytes())?;
        stdout.flush()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if self.active {
            self.active = false;
            let mut stdout = io::stdout().lock();
            let _ = stdout.write_all(
                b"\x1b[?2026l\x1b[?2004l\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b]112\x07\x1b[0 q\x1b[?25h\x1b[?1049l",
            );
            let _ = stdout.flush();
            unsafe {
                libc::tcsetattr(self.fd, libc::TCSAFLUSH, &self.orig);
            }
        }
    }
}

pub fn copy_to_system_clipboard(text: &str) {
    if text.is_empty() {
        return;
    }
    let candidates: [(&str, &[&str]); 4] = [
        ("clip.exe", &[]),
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("pbcopy", &[]),
    ];
    for (cmd, args) in candidates {
        if let Ok(mut child) = Command::new(cmd)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return;
        }
    }
}

pub fn paste_from_system_clipboard() -> Option<String> {
    let candidates: [(&str, &[&str]); 4] = [
        ("powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", "Get-Clipboard"]),
        ("wl-paste", &["-n"]),
        ("xclip", &["-o", "-selection", "clipboard"]),
        ("pbpaste", &[]),
    ];
    for (cmd, args) in candidates {
        if let Ok(out) = Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
                let trimmed = s.strip_suffix('\n').unwrap_or(&s).to_string();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
        }
    }
    None
}

/// Manages the optional native Windows `nave-hidemouse.exe` helper on WSL2.
/// Auto-hides the OS mouse pointer after 500ms of mouse stillness (or immediately on keypress)
/// ONLY while Windows Terminal is foreground and the pointer sits inside the editor canvas.
/// Automatically restores Windows system cursors on movement, focus loss, toggle OFF, or process exit.
pub struct MouseHider {
    exe_path: Option<PathBuf>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    last_notify: Option<Instant>,
}

impl MouseHider {
    pub fn new() -> Self {
        Self {
            exe_path: Self::locate_helper(),
            child: None,
            stdin: None,
            last_notify: None,
        }
    }

    fn locate_helper() -> Option<PathBuf> {
        if let Ok(env_p) = std::env::var("NAVE_HIDEMOUSE_EXE") {
            let p = PathBuf::from(env_p);
            if p.is_file() {
                return Some(p);
            }
        }
        if let Ok(user) = std::env::var("USER") {
            let p = PathBuf::from(format!("/mnt/c/Users/{}/.config/nave/nave-hidemouse.exe", user));
            if p.is_file() {
                return Some(p);
            }
        }
        if let Ok(entries) = std::fs::read_dir("/mnt/c/Users") {
            for ent in entries.flatten() {
                let cand = ent.path().join(".config/nave/nave-hidemouse.exe");
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
        None
    }

    pub fn sync(&mut self, active: bool) {
        if active {
            if self.child.is_some() {
                return;
            }
            let Some(ref exe) = self.exe_path else { return };
            if let Ok(mut child) = Command::new(exe)
                .arg("500")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                self.stdin = child.stdin.take();
                self.child = Some(child);
                self.last_notify = None;
            }
        } else {
            if let Some(mut s) = self.stdin.take() {
                let _ = s.write_all(b"Q");
                let _ = s.flush();
            }
            if let Some(mut child) = self.child.take() {
                let _ = child.wait();
            }
            self.last_notify = None;
        }
    }

    pub fn notify_key(&mut self) {
        let Some(ref mut s) = self.stdin else { return };
        if self
            .last_notify
            .is_some_and(|t| t.elapsed() < Duration::from_millis(120))
        {
            return;
        }
        self.last_notify = Some(Instant::now());
        if s.write_all(b"H").is_err() || s.flush().is_err() {
            self.stdin = None;
        }
    }
}

impl Drop for MouseHider {
    fn drop(&mut self) {
        self.sync(false);
    }
}
