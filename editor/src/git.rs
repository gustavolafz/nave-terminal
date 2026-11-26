use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GutterMark {
    Added,
    Modified,
    Deleted,
}

pub struct GitGutter {
    marks: Arc<Mutex<HashMap<usize, GutterMark>>>,
    dirty: Arc<AtomicBool>,
}

impl GitGutter {
    pub fn new() -> Self {
        Self {
            marks: Arc::new(Mutex::new(HashMap::new())),
            dirty: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn refresh_async(&self, path: &Path) {
        let path_buf: PathBuf = path.to_path_buf();
        let marks = Arc::clone(&self.marks);
        let dirty = Arc::clone(&self.dirty);

        thread::spawn(move || {
            let computed = compute_git_diff_marks(&path_buf);
            if let Ok(mut guard) = marks.lock() {
                *guard = computed;
            }
            dirty.store(true, Ordering::SeqCst);
        });
    }

    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::SeqCst)
    }

    pub fn snapshot(&self) -> HashMap<usize, GutterMark> {
        self.marks.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

fn compute_git_diff_marks(path: &Path) -> HashMap<usize, GutterMark> {
    let mut map = HashMap::new();
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let out = match Command::new("git")
        .current_dir(parent)
        .args(["diff", "--unified=0", "--no-color", "HEAD", "--"])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return map,
    };

    let diff = String::from_utf8_lossy(&out.stdout);
    for line in diff.lines() {
        if !line.starts_with("@@ ") {
            continue;
        }
        // Parse `@@ -old_start[,old_count] +new_start[,new_count] @@`
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let (old_count, new_start, new_count) = match (
            parse_hunk_range(parts[1].trim_start_matches('-')),
            parse_hunk_range(parts[2].trim_start_matches('+')),
        ) {
            (Some((_, oc)), Some((ns, nc))) => (oc, ns, nc),
            _ => continue,
        };

        if new_count == 0 {
            let row = new_start.saturating_sub(1);
            map.insert(row, GutterMark::Deleted);
        } else {
            let mark = if old_count == 0 {
                GutterMark::Added
            } else {
                GutterMark::Modified
            };
            let start_row = new_start.saturating_sub(1);
            for r in start_row..(start_row + new_count) {
                map.insert(r, mark);
            }
        }
    }
    map
}

fn parse_hunk_range(spec: &str) -> Option<(usize, usize)> {
    if let Some((a, b)) = spec.split_once(',') {
        Some((a.parse().ok()?, b.parse().ok()?))
    } else {
        Some((spec.parse().ok()?, 1))
    }
}
