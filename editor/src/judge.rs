use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagSeverity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: usize, // 0-based
    pub col: usize,  // 0-based
    pub severity: DiagSeverity,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct TestCase {
    pub label: String,
    pub input: String,
    pub expected: Option<String>,
    pub expected_name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct JudgeResult {
    pub src_path: PathBuf,
    pub summary: String,
    pub report: String,
    pub diagnostics: Vec<Diagnostic>,
    pub updated_in_txt: Option<PathBuf>,
}

#[derive(Default)]
struct SharedState {
    progress: Option<String>,
    result: Option<JudgeResult>,
}

pub struct JudgeRunner {
    shared: Arc<Mutex<SharedState>>,
    dirty: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    cancel_flag: Arc<AtomicBool>,
}

impl JudgeRunner {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(SharedState::default())),
            dirty: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    pub fn current_progress(&self) -> Option<String> {
        self.shared.lock().ok().and_then(|s| s.progress.clone())
    }

    pub fn take_result(&self) -> Option<JudgeResult> {
        self.shared.lock().ok().and_then(|mut s| s.result.take())
    }

    pub fn start_judge(&self, src_path: PathBuf, debug_mode: bool, timeout_ms: u64) {
        self.cancel_flag.store(false, Ordering::Relaxed);
        self.running.store(true, Ordering::Relaxed);
        if let Ok(mut st) = self.shared.lock() {
            st.progress = Some(if debug_mode {
                "Compilando [DEBUG ASan+UBSan]...".into()
            } else {
                "Compilando [O2 C++20 -DLOCAL]...".into()
            });
            st.result = None;
        }
        self.dirty.store(true, Ordering::Release);

        let shared = Arc::clone(&self.shared);
        let dirty = Arc::clone(&self.dirty);
        let running = Arc::clone(&self.running);
        let cancel = Arc::clone(&self.cancel_flag);

        thread::spawn(move || {
            let res = run_judge_sync(&src_path, debug_mode, timeout_ms, &shared, &dirty, &cancel);
            if let Ok(mut st) = shared.lock() {
                st.progress = None;
                st.result = Some(res);
            }
            running.store(false, Ordering::Relaxed);
            dirty.store(true, Ordering::Release);
        });
    }

    pub fn start_stress_test(&self, sol_path: PathBuf, timeout_ms: u64, max_iters: usize) {
        self.cancel_flag.store(false, Ordering::Relaxed);
        self.running.store(true, Ordering::Relaxed);
        if let Ok(mut st) = self.shared.lock() {
            st.progress = Some("Stress Test: compilando sol + brute + gen...".into());
            st.result = None;
        }
        self.dirty.store(true, Ordering::Release);

        let shared = Arc::clone(&self.shared);
        let dirty = Arc::clone(&self.dirty);
        let running = Arc::clone(&self.running);
        let cancel = Arc::clone(&self.cancel_flag);

        thread::spawn(move || {
            let res = run_stress_sync(&sol_path, timeout_ms, max_iters, &shared, &dirty, &cancel);
            if let Ok(mut st) = shared.lock() {
                st.progress = None;
                st.result = Some(res);
            }
            running.store(false, Ordering::Relaxed);
            dirty.store(true, Ordering::Release);
        });
    }
}

fn set_progress(shared: &Arc<Mutex<SharedState>>, dirty: &Arc<AtomicBool>, msg: impl Into<String>) {
    if let Ok(mut st) = shared.lock() {
        st.progress = Some(msg.into());
    }
    dirty.store(true, Ordering::Release);
}

struct ExecOutcome {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    elapsed_ms: f64,
    peak_kb: u64,
    tle: bool,
}

fn exec_with_timeout(
    program: &str,
    args: &[&str],
    cwd: &Path,
    stdin_data: &str,
    timeout_ms: u64,
    cancel: &AtomicBool,
) -> io::Result<ExecOutcome> {
    let t0 = Instant::now();
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let pid = child.id();

    if let Some(mut stdin) = child.stdin.take() {
        let data = stdin_data.as_bytes().to_vec();
        thread::spawn(move || {
            let _ = stdin.write_all(&data);
        });
    }

    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_handle = thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut p) = out_pipe {
            let _ = p.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    });
    let err_handle = thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut p) = err_pipe {
            let _ = p.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    });

    let deadline = Duration::from_millis(timeout_ms.max(200));
    let mut peak_kb = 0u64;
    let mut tle = false;
    let mut exit_code = None;

    loop {
        if let Some(kb) = read_proc_peak_kb(pid) {
            peak_kb = peak_kb.max(kb);
        }
        match child.try_wait()? {
            Some(status) => {
                exit_code = status.code();
                break;
            }
            None => {
                if cancel.load(Ordering::Relaxed) || t0.elapsed() >= deadline {
                    tle = !cancel.load(Ordering::Relaxed);
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                thread::sleep(Duration::from_millis(4));
            }
        }
    }

    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();

    Ok(ExecOutcome {
        exit_code,
        stdout,
        stderr,
        elapsed_ms,
        peak_kb,
        tle,
    })
}

fn read_proc_peak_kb(pid: u32) -> Option<u64> {
    let raw = fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:").or_else(|| line.strip_prefix("VmRSS:")) {
            if let Some(num) = rest.split_whitespace().next().and_then(|s| s.parse::<u64>().ok()) {
                return Some(num);
            }
        }
    }
    None
}

fn normalize_judge_output(s: &str) -> Vec<String> {
    let mut lines: Vec<String> = s
        .replace("\r\n", "\n")
        .lines()
        .map(|l| l.trim_end().to_string())
        .collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

pub fn outputs_match(actual: &str, expected: &str) -> bool {
    normalize_judge_output(actual) == normalize_judge_output(expected)
}

fn split_multi_cases(raw: &str) -> Vec<String> {
    let norm = raw.replace("\r\n", "\n");
    if !norm.lines().any(|l| l.trim() == "---") {
        return vec![norm];
    }
    let mut cases = Vec::new();
    let mut cur = Vec::new();
    for line in norm.lines() {
        if line.trim() == "---" {
            let joined = cur.join("\n");
            if !joined.trim().is_empty() {
                cases.push(joined + "\n");
            }
            cur.clear();
        } else {
            cur.push(line);
        }
    }
    let joined = cur.join("\n");
    if !joined.trim().is_empty() {
        cases.push(joined + "\n");
    }
    if cases.is_empty() {
        vec![String::new()]
    } else {
        cases
    }
}

pub fn is_runnable_source(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(ext.as_str(), "cpp" | "cc" | "cxx" | "c" | "rs" | "py" | "js" | "mjs" | "sh" | "zsh")
}

/// Resolve the true working directory of `src_path`, avoiding Rust's `Path::new("sol.cpp").parent() == Some("")` pitfall.
pub fn resolve_src_dir(src_path: &Path) -> PathBuf {
    if let Some(parent) = src_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        if parent.is_absolute() {
            parent.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(parent))
                .unwrap_or_else(|_| parent.to_path_buf())
        }
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }
}

fn read_non_empty(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    if raw.trim().is_empty() {
        None
    } else {
        Some(raw)
    }
}

fn first_diff_line(actual: &str, expected: &str) -> Option<usize> {
    let a = normalize_judge_output(actual);
    let e = normalize_judge_output(expected);
    let n = a.len().max(e.len());
    for i in 0..n {
        if a.get(i) != e.get(i) {
            return Some(i + 1);
        }
    }
    None
}

#[cfg(test)]
pub fn discover_test_cases(dir: &Path) -> Vec<TestCase> {
    discover_test_cases_for(dir, None)
}

pub fn discover_test_cases_for(dir: &Path, src_stem: Option<&str>) -> Vec<TestCase> {
    let mut cases: Vec<TestCase> = Vec::new();

    // 1. Primary `in.txt` (supports multi-case `---` splits!) + `expected.txt` (or `ans.txt` / `out.txt`)
    let in_p = dir.join("in.txt");
    let exp_txt = dir.join("expected.txt");
    let (exp_name, exp_raw): (Option<String>, Option<String>) = if exp_txt.is_file() {
        // When `expected.txt` exists, prefer it strictly over stale `out.txt`; if blank, treat as no expected yet.
        (Some("expected.txt".into()), read_non_empty(&exp_txt))
    } else if let Some((name, content)) = ["ans.txt", "out.txt"]
        .iter()
        .find_map(|&n| read_non_empty(&dir.join(n)).map(|c| (n.to_string(), c)))
    {
        (Some(name), Some(content))
    } else {
        (None, None)
    };

    if in_p.is_file() {
        let raw_in = fs::read_to_string(&in_p).unwrap_or_default();
        let ins = split_multi_cases(&raw_in);
        let exps = exp_raw.as_deref().map(split_multi_cases);
        let multi = ins.len() > 1;
        for (i, inp) in ins.into_iter().enumerate() {
            let exp = exps
                .as_ref()
                .and_then(|v| v.get(i))
                .filter(|s| !s.trim().is_empty())
                .cloned();
            let label = match (&exp, &exp_name) {
                (Some(_), Some(en)) if multi => format!("Case #{} (in.txt ↔ {})", i + 1, en),
                (Some(_), Some(en)) => format!("Case #1 (in.txt ↔ {})", en),
                _ if multi => format!("Case #{} (in.txt)", i + 1),
                _ => "Case #1 (in.txt)".into(),
            };
            cases.push(TestCase {
                label,
                input: inp,
                expected: exp,
                expected_name: exp_name.clone(),
            });
        }
    }

    // 2. Numbered cases: in1.txt..in20.txt + expected{i}.txt / ans{i}.txt / out{i}.txt
    for idx in 1..=20 {
        let in_num = dir.join(format!("in{}.txt", idx));
        if !in_num.is_file() {
            continue;
        }
        let input = fs::read_to_string(&in_num).unwrap_or_default();
        let exp_found = [
            format!("expected{}.txt", idx),
            format!("ans{}.txt", idx),
            format!("out{}.txt", idx),
        ]
        .into_iter()
        .find_map(|n| read_non_empty(&dir.join(&n)).map(|c| (n, c)));
        let (ename, exp) = match exp_found {
            Some((n, c)) => (Some(n), Some(c)),
            None => (None, None),
        };
        let label = match &ename {
            Some(en) => format!("Case #{} (in{}.txt ↔ {})", cases.len() + 1, idx, en),
            None => format!("Case #{} (in{}.txt)", cases.len() + 1, idx),
        };
        cases.push(TestCase {
            label,
            input,
            expected: exp,
            expected_name: ename,
        });
    }

    if !cases.is_empty() {
        return cases;
    }

    // 3. Stem-based files (e.g. `A.in` + `A.expected` / `A.ans` / `A.out`)
    if let Some(stem) = src_stem {
        let stem_in = dir.join(format!("{}.in", stem));
        if stem_in.is_file() {
            let input = fs::read_to_string(&stem_in).unwrap_or_default();
            let exp_found = [
                format!("{}.expected", stem),
                format!("{}.ans", stem),
                format!("{}.out", stem),
            ]
            .into_iter()
            .find_map(|n| read_non_empty(&dir.join(&n)).map(|c| (n, c)));
            let (ename, exp) = match exp_found {
                Some((n, c)) => (Some(n), Some(c)),
                None => (None, None),
            };
            let label = match &ename {
                Some(en) => format!("Case #1 ({}.in ↔ {})", stem, en),
                None => format!("Case #1 ({}.in)", stem),
            };
            return vec![TestCase {
                label,
                input,
                expected: exp,
                expected_name: ename,
            }];
        }
    }

    // 4. Fallback: no input file -> single empty-stdin execution (still checks expected.txt if filled!)
    let label = match (&exp_raw, &exp_name) {
        (Some(_), Some(en)) => format!("Run #1 (sem in.txt ↔ {})", en),
        _ => "Run #1 (sem in.txt)".into(),
    };
    vec![TestCase {
        label,
        input: String::new(),
        expected: exp_raw,
        expected_name: exp_name,
    }]
}

pub fn parse_compiler_diagnostics(stderr: &str, src_name: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut pending_rust_msg: Option<(DiagSeverity, String)> = None;

    for line in stderr.lines() {
        let trimmed = line.trim();

        // Rustc style: `error[E...]: msg` followed by `--> file:line:col`
        if let Some(rest) = trimmed.strip_prefix("error") {
            if let Some((_, msg)) = rest.split_once(':') {
                pending_rust_msg = Some((DiagSeverity::Error, msg.trim().to_string()));
                continue;
            }
        } else if let Some(rest) = trimmed.strip_prefix("warning") {
            if let Some((_, msg)) = rest.split_once(':') {
                pending_rust_msg = Some((DiagSeverity::Warning, msg.trim().to_string()));
                continue;
            }
        }
        if let Some(loc) = trimmed.strip_prefix("--> ") {
            if let Some((sev, msg)) = pending_rust_msg.take() {
                let parts: Vec<&str> = loc.rsplitn(3, ':').collect();
                if parts.len() == 3 {
                    if let (Ok(col), Ok(ln)) = (parts[0].trim().parse::<usize>(), parts[1].trim().parse::<usize>()) {
                        out.push(Diagnostic {
                            line: ln.saturating_sub(1),
                            col: col.saturating_sub(1),
                            severity: sev,
                            message: msg,
                        });
                        continue;
                    }
                }
            }
        }

        // Python style: `File "...sol.py", line 12`
        if trimmed.starts_with("File \"") && trimmed.contains(", line ") {
            if let Some((_, after)) = trimmed.split_once(", line ") {
                let num_s: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(ln) = num_s.parse::<usize>() {
                    out.push(Diagnostic {
                        line: ln.saturating_sub(1),
                        col: 0,
                        severity: DiagSeverity::Error,
                        message: trimmed.to_string(),
                    });
                    continue;
                }
            }
        }

        // GCC / Clang style: `sol.cpp:14:9: error: expected ';'`
        if let Some(pos) = line.find(src_name) {
            let after_file = &line[pos + src_name.len()..];
            if let Some(rest) = after_file.strip_prefix(':') {
                let mut segs = rest.splitn(4, ':');
                let ln_s = segs.next().unwrap_or("").trim();
                let col_s = segs.next().unwrap_or("").trim();
                let kind_s = segs.next().unwrap_or("").trim();
                let msg_s = segs.next().unwrap_or("").trim();
                if let Ok(ln) = ln_s.parse::<usize>() {
                    let col = col_s.parse::<usize>().unwrap_or(1);
                    let sev = if kind_s.contains("error") {
                        Some(DiagSeverity::Error)
                    } else if kind_s.contains("warning") {
                        Some(DiagSeverity::Warning)
                    } else {
                        None
                    };
                    if let Some(severity) = sev {
                        out.push(Diagnostic {
                            line: ln.saturating_sub(1),
                            col: col.saturating_sub(1),
                            severity,
                            message: format!("{}: {}", kind_s, msg_s),
                        });
                    }
                }
            }
        }
    }
    out
}

fn compile_source(
    src_path: &Path,
    bin_path: &Path,
    debug_mode: bool,
) -> Result<(Vec<Diagnostic>, String), (Vec<Diagnostic>, String)> {
    let ext = src_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let src_name = src_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("source");
    let src_str = src_path.to_string_lossy().into_owned();
    let bin_str = bin_path.to_string_lossy().into_owned();

    let (compiler, args): (&str, Vec<String>) = match ext.as_str() {
        "cpp" | "cc" | "cxx" => {
            if debug_mode {
                (
                    "g++",
                    vec![
                        "-O1".into(),
                        "-g".into(),
                        "-std=c++20".into(),
                        "-Wall".into(),
                        "-Wextra".into(),
                        "-DLOCAL".into(),
                        "-D_GLIBCXX_DEBUG".into(),
                        "-fsanitize=address,undefined".into(),
                        src_str,
                        "-o".into(),
                        bin_str,
                    ],
                )
            } else {
                (
                    "g++",
                    vec![
                        "-O2".into(),
                        "-std=c++20".into(),
                        "-Wall".into(),
                        "-Wextra".into(),
                        "-DLOCAL".into(),
                        src_str,
                        "-o".into(),
                        bin_str,
                    ],
                )
            }
        }
        "c" => {
            let mut a = vec![
                if debug_mode { "-O1" } else { "-O2" }.into(),
                "-std=c17".into(),
                "-Wall".into(),
                "-Wextra".into(),
                "-DLOCAL".into(),
            ];
            if debug_mode {
                a.push("-g".into());
                a.push("-fsanitize=address,undefined".into());
            }
            a.push(src_str);
            a.push("-o".into());
            a.push(bin_str);
            ("gcc", a)
        }
        "rs" => (
            "rustc",
            vec![
                if debug_mode { "-g" } else { "-O" }.into(),
                src_str,
                "-o".into(),
                bin_str,
            ],
        ),
        _ => return Ok((Vec::new(), String::new())),
    };

    let output = Command::new(compiler)
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| (Vec::new(), format!("Falha ao invocar {}: {}", compiler, e)))?;

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let diags = parse_compiler_diagnostics(&stderr, src_name);
    if output.status.success() {
        Ok((diags, stderr))
    } else {
        Err((diags, stderr))
    }
}

fn run_judge_sync(
    src_path: &Path,
    debug_mode: bool,
    timeout_ms: u64,
    shared: &Arc<Mutex<SharedState>>,
    dirty: &Arc<AtomicBool>,
    cancel: &AtomicBool,
) -> JudgeResult {
    let dir_owned = resolve_src_dir(src_path);
    let dir = dir_owned.as_path();
    let src_name = src_path.file_name().and_then(|s| s.to_str()).unwrap_or("source");
    let src_stem = src_path.file_stem().and_then(|s| s.to_str());
    let ext = src_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let mode_lbl = if debug_mode { "DEBUG (ASan+UBSan)" } else { "RELEASE (O2)" };
    let needs_compile = matches!(ext.as_str(), "cpp" | "cc" | "cxx" | "c" | "rs");
    let bin_path = std::env::temp_dir().join(format!("nave_judge_{}", std::process::id()));

    let mut report = String::new();
    report.push_str(&format!(
        "═══ NAVE JUDGE · {} · [{}] · TL={}ms ═══\n",
        src_name, mode_lbl, timeout_ms
    ));

    let mut diagnostics = Vec::new();
    if needs_compile {
        let t_comp = Instant::now();
        match compile_source(src_path, &bin_path, debug_mode) {
            Ok((diags, warn_txt)) => {
                diagnostics = diags;
                let cms = t_comp.elapsed().as_secs_f64() * 1000.0;
                report.push_str(&format!("✓ Compilado em {:.0} ms", cms));
                if !diagnostics.is_empty() {
                    report.push_str(&format!(" ({} warning(s) · F8 navegar)", diagnostics.len()));
                }
                report.push('\n');
                if !warn_txt.trim().is_empty() {
                    report.push_str("--- Warnings ---\n");
                    report.push_str(warn_txt.trim());
                    report.push('\n');
                }
            }
            Err((diags, err_txt)) => {
                let _ = fs::remove_file(&bin_path);
                let n_err = diags.len();
                report.push_str("× ERRO DE COMPILACAO (CE)\n\n");
                report.push_str(&err_txt);
                let summary = if let Some(first) = diags.first() {
                    format!(
                        "× CE Ln {}: {} (F8 prox · {} diag)",
                        first.line + 1,
                        first.message,
                        n_err
                    )
                } else {
                    "× CE: Erro de compilacao (veja painel [Judge])".into()
                };
                return JudgeResult {
                    src_path: src_path.to_path_buf(),
                    summary,
                    report,
                    diagnostics: diags,
                    updated_in_txt: None,
                };
            }
        }
    }

    // Discover cases in the source file's directory; if no in.txt/in1.txt there, also check CWD
    let mut cases = discover_test_cases_for(dir, src_stem);
    let found_in_src_dir = !cases.first().is_some_and(|c| c.label.starts_with("Run #1 (sem in.txt"));
    if !found_in_src_dir {
        if let Ok(cwd) = std::env::current_dir() {
            if cwd != dir {
                let cwd_cases = discover_test_cases_for(&cwd, src_stem);
                if !cwd_cases.first().is_some_and(|c| c.label.starts_with("Run #1 (sem in.txt")) {
                    cases = cwd_cases;
                }
            }
        }
    }
    let mut ac_count = 0usize;
    let mut run_ok_count = 0usize;
    let mut max_ms = 0.0f64;
    let mut max_kb = 0u64;
    let mut any_fail = false;

    for (idx, tc) in cases.iter().enumerate() {
        set_progress(
            shared,
            dirty,
            format!("Executando {}/{} ({})...", idx + 1, cases.len(), tc.label),
        );

        let src_s = src_path.to_string_lossy().into_owned();
        let bin_s = bin_path.to_string_lossy().into_owned();
        let outcome = if needs_compile {
            exec_with_timeout(&bin_s, &[], dir, &tc.input, timeout_ms, cancel)
        } else {
            match ext.as_str() {
                "py" => exec_with_timeout("python3", &[&src_s], dir, &tc.input, timeout_ms, cancel),
                "js" | "mjs" => exec_with_timeout("node", &[&src_s], dir, &tc.input, timeout_ms, cancel),
                "sh" | "zsh" => exec_with_timeout("bash", &[&src_s], dir, &tc.input, timeout_ms, cancel),
                _ => {
                    report.push_str(&format!("Extensao nao suportada para execucao: .{}\n", ext));
                    any_fail = true;
                    break;
                }
            }
        };

        report.push_str(&format!("\n─── {} ───\n", tc.label));
        match outcome {
            Err(e) => {
                any_fail = true;
                report.push_str(&format!("× Erro ao executar: {}\n", e));
            }
            Ok(out) => {
                max_ms = max_ms.max(out.elapsed_ms);
                max_kb = max_kb.max(out.peak_kb);
                let mem_mb = out.peak_kb as f64 / 1024.0;
                if out.tle {
                    any_fail = true;
                    report.push_str(&format!(
                        "⏱ TLE (> {} ms) · {:.1} MB\n",
                        timeout_ms, mem_mb
                    ));
                } else if out.exit_code != Some(0) {
                    any_fail = true;
                    let code_s = out
                        .exit_code
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "SIGNAL".into());
                    report.push_str(&format!(
                        "💥 RE (exit {}) · {:.1} ms · {:.1} MB\n",
                        code_s, out.elapsed_ms, mem_mb
                    ));
                    if !out.stderr.trim().is_empty() {
                        let extra = parse_compiler_diagnostics(&out.stderr, src_name);
                        diagnostics.extend(extra);
                        report.push_str("--- stderr / sanitizer ---\n");
                        report.push_str(out.stderr.trim());
                        report.push('\n');
                    }
                } else {
                    run_ok_count += 1;
                    if let Some(ref exp) = tc.expected {
                        if outputs_match(&out.stdout, exp) {
                            ac_count += 1;
                            report.push_str(&format!(
                                "✓ AC · {:.1} ms · {:.1} MB\n",
                                out.elapsed_ms, mem_mb
                            ));
                            let out_lines: Vec<&str> = out.stdout.trim_end().lines().collect();
                            if !out_lines.is_empty() {
                                for ln in out_lines.iter().take(12) {
                                    report.push_str("  ");
                                    report.push_str(ln);
                                    report.push('\n');
                                }
                                if out_lines.len() > 12 {
                                    report.push_str(&format!("  … (+{} linhas)\n", out_lines.len() - 12));
                                }
                            }
                        } else {
                            any_fail = true;
                            let diff_hint = first_diff_line(&out.stdout, exp)
                                .map(|n| format!(" · divergencia na linha {}", n))
                                .unwrap_or_default();
                            report.push_str(&format!(
                                "× WA{} · {:.1} ms · {:.1} MB\n",
                                diff_hint, out.elapsed_ms, mem_mb
                            ));
                            let exp_hdr = tc
                                .expected_name
                                .as_deref()
                                .map(|n| format!("Esperado ({}):\n", n))
                                .unwrap_or_else(|| "Esperado:\n".into());
                            report.push_str(&exp_hdr);
                            report.push_str(exp.trim_end());
                            report.push_str("\nObtido:\n");
                            report.push_str(out.stdout.trim_end());
                            report.push('\n');
                        }
                    } else {
                        let empty_note = if tc.expected_name.as_deref() == Some("expected.txt") {
                            " (expected.txt vazio)"
                        } else {
                            ""
                        };
                        report.push_str(&format!(
                            "✓ OK{} · {:.1} ms · {:.1} MB\n",
                            empty_note, out.elapsed_ms, mem_mb
                        ));
                        report.push_str(&out.stdout);
                        if !out.stdout.ends_with('\n') && !out.stdout.is_empty() {
                            report.push('\n');
                        }
                    }
                    if !out.stderr.trim().is_empty() {
                        report.push_str("--- dbg (stderr) ---\n");
                        report.push_str(out.stderr.trim());
                        report.push('\n');
                    }
                }
            }
        }
    }

    if needs_compile {
        let _ = fs::remove_file(&bin_path);
    }

    let has_expected = cases.iter().any(|c| c.expected.is_some());
    let mem_mb = max_kb as f64 / 1024.0;
    let summary = if !any_fail && has_expected {
        format!(
            "✓ AC {}/{} · max {:.1} ms · {:.1} MB",
            ac_count,
            cases.len(),
            max_ms,
            mem_mb
        )
    } else if !any_fail {
        format!(
            "✓ OK ({} run) · {:.1} ms · {:.1} MB",
            run_ok_count, max_ms, mem_mb
        )
    } else if has_expected {
        format!(
            "× Falhou ({}/{} AC) · max {:.1} ms · veja [Judge]",
            ac_count,
            cases.len(),
            max_ms
        )
    } else {
        format!("× Falhou na execucao · max {:.1} ms · veja [Judge]", max_ms)
    };

    JudgeResult {
        src_path: src_path.to_path_buf(),
        summary,
        report,
        diagnostics,
        updated_in_txt: None,
    }
}

pub fn scaffold_stress_files(dir: &Path) -> io::Result<(PathBuf, PathBuf)> {
    let gen_p = dir.join("gen.cpp");
    let brute_p = dir.join("brute.cpp");

    if !gen_p.exists() {
        fs::write(
            &gen_p,
            "#include <bits/stdc++.h>\n\
             using namespace std;\n\n\
             int main(int argc, char** argv) {\n\
             \x20   uint64_t seed = argc > 1 ? stoull(argv[1]) : 1;\n\
             \x20   mt19937_64 rng(seed);\n\
             \x20   auto rand_int = [&](long long l, long long r) {\n\
             \x20       return uniform_int_distribution<long long>(l, r)(rng);\n\
             \x20   };\n\
             \x20   int n = (int)rand_int(1, 10);\n\
             \x20   cout << n << '\\n';\n\
             \x20   for (int i = 0; i < n; i++) {\n\
             \x20       cout << rand_int(1, 20) << (i + 1 == n ? '\\n' : ' ');\n\
             \x20   }\n\
             \x20   return 0;\n\
             }\n",
        )?;
    }
    if !brute_p.exists() {
        fs::write(
            &brute_p,
            "#include <bits/stdc++.h>\n\
             using namespace std;\n\n\
             int main() {\n\
             \x20   ios::sync_with_stdio(false);\n\
             \x20   cin.tie(nullptr);\n\
             \x20   int n;\n\
             \x20   if (!(cin >> n)) return 0;\n\
             \x20   vector<long long> a(n);\n\
             \x20   for (auto& x : a) cin >> x;\n\
             \x20   // TODO: implemente a solucao forca-bruta O(N^2) garantidamente correta\n\
             \x20   long long ans = 0;\n\
             \x20   for (auto x : a) ans += x;\n\
             \x20   cout << ans << '\\n';\n\
             \x20   return 0;\n\
             }\n",
        )?;
    }
    Ok((gen_p, brute_p))
}

fn run_stress_sync(
    sol_path: &Path,
    timeout_ms: u64,
    max_iters: usize,
    shared: &Arc<Mutex<SharedState>>,
    dirty: &Arc<AtomicBool>,
    cancel: &AtomicBool,
) -> JudgeResult {
    let dir_owned = resolve_src_dir(sol_path);
    let dir = dir_owned.as_path();
    let gen_path = dir.join("gen.cpp");
    let brute_path = dir.join("brute.cpp");

    if !gen_path.is_file() || !brute_path.is_file() {
        return JudgeResult {
            src_path: sol_path.to_path_buf(),
            summary: "Faltam gen.cpp e/ou brute.cpp (use F1 -> Gerar Esqueleto Stress)".into(),
            report: "Stress Tester requer 'gen.cpp' e 'brute.cpp' na mesma pasta.\n\
                     Use F1 -> 'CP: Gerar Esqueleto Stress (gen.cpp + brute.cpp)' para cria-los.\n"
                .into(),
            diagnostics: Vec::new(),
            updated_in_txt: None,
        };
    }

    let pid = std::process::id();
    let sol_bin = std::env::temp_dir().join(format!("nave_stress_sol_{}", pid));
    let brute_bin = std::env::temp_dir().join(format!("nave_stress_brute_{}", pid));
    let gen_bin = std::env::temp_dir().join(format!("nave_stress_gen_{}", pid));

    let cleanup = || {
        let _ = fs::remove_file(&sol_bin);
        let _ = fs::remove_file(&brute_bin);
        let _ = fs::remove_file(&gen_bin);
    };

    for (label, src, bin) in [
        ("sol", sol_path, &sol_bin),
        ("brute", brute_path.as_path(), &brute_bin),
        ("gen", gen_path.as_path(), &gen_bin),
    ] {
        set_progress(shared, dirty, format!("Stress: compilando {}...", label));
        if let Err((diags, err)) = compile_source(src, bin, false) {
            cleanup();
            return JudgeResult {
                src_path: src.to_path_buf(),
                summary: format!("× Stress CE em {}", label),
                report: format!("═══ STRESS TESTER · Erro de compilacao em {} ═══\n\n{}", label, err),
                diagnostics: if label == "sol" { diags } else { Vec::new() },
                updated_in_txt: None,
            };
        }
    }

    let sol_s = sol_bin.to_string_lossy().into_owned();
    let brute_s = brute_bin.to_string_lossy().into_owned();
    let gen_s = gen_bin.to_string_lossy().into_owned();

    let t0 = Instant::now();
    for iter in 1..=max_iters {
        if cancel.load(Ordering::Relaxed) {
            cleanup();
            return JudgeResult {
                src_path: sol_path.to_path_buf(),
                summary: format!("Stress interrompido apos {} testes", iter - 1),
                report: format!("Stress test cancelado pelo usuario no teste #{}.\n", iter),
                diagnostics: Vec::new(),
                updated_in_txt: None,
            };
        }
        if iter == 1 || iter % 10 == 0 {
            set_progress(shared, dirty, format!("Stress Test #{}/{} ✓", iter, max_iters));
        }

        let seed_s = iter.to_string();
        let Ok(gen_out) = exec_with_timeout(&gen_s, &[&seed_s], dir, "", timeout_ms, cancel) else {
            break;
        };
        if gen_out.tle || gen_out.exit_code != Some(0) {
            cleanup();
            return JudgeResult {
                src_path: sol_path.to_path_buf(),
                summary: format!("× Stress: gen.cpp falhou na seed {}", iter),
                report: format!("gen.cpp falhou (seed {}):\n{}", iter, gen_out.stderr),
                diagnostics: Vec::new(),
                updated_in_txt: None,
            };
        }

        let Ok(brute_out) = exec_with_timeout(&brute_s, &[], dir, &gen_out.stdout, timeout_ms, cancel) else {
            break;
        };
        let Ok(sol_out) = exec_with_timeout(&sol_s, &[], dir, &gen_out.stdout, timeout_ms, cancel) else {
            break;
        };

        let failed = sol_out.tle
            || sol_out.exit_code != Some(0)
            || !outputs_match(&sol_out.stdout, &brute_out.stdout);

        if failed {
            let in_path = dir.join("in.txt");
            let _ = fs::write(&in_path, &gen_out.stdout);
            cleanup();
            let reason = if sol_out.tle {
                format!("TLE (> {} ms)", timeout_ms)
            } else if sol_out.exit_code != Some(0) {
                format!("RE (exit {:?})", sol_out.exit_code)
            } else {
                "WA (divergiu de brute.cpp)".into()
            };
            let report = format!(
                "═══ STRESS TESTER · CONTRA-EXEMPLO ENCONTRADO! ═══\n\
                 Seed: #{} · Veredito: {}\n\
                 ✓ Entrada gravada automaticamente em in.txt!\n\n\
                 ─── Entrada (in.txt) ───\n{}\n\
                 ─── Esperado (brute.cpp) ───\n{}\n\
                 ─── Obtido (sol) ───\n{}\n{}",
                iter,
                reason,
                gen_out.stdout.trim_end(),
                brute_out.stdout.trim_end(),
                sol_out.stdout.trim_end(),
                if sol_out.stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!("\n─── stderr ───\n{}\n", sol_out.stderr.trim())
                }
            );
            return JudgeResult {
                src_path: sol_path.to_path_buf(),
                summary: format!("× Contra-exemplo no teste #{} ({}) → salvo em in.txt!", iter, reason),
                report,
                diagnostics: Vec::new(),
                updated_in_txt: Some(in_path),
            };
        }
    }

    cleanup();
    let secs = t0.elapsed().as_secs_f64();
    JudgeResult {
        src_path: sol_path.to_path_buf(),
        summary: format!("✓ STRESS PASSOU: {}/{} testes ({:.2}s)", max_iters, max_iters, secs),
        report: format!(
            "═══ STRESS TESTER · 100% APROVADO ═══\n\
             ✓ Todos os {} casos aleatorios (seeds 1..={}) bateram com brute.cpp!\n\
             Tempo total: {:.2}s\n",
            max_iters, max_iters, secs
        ),
        diagnostics: Vec::new(),
        updated_in_txt: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gcc_and_rustc_diagnostics() {
        let gcc = "sol.cpp:14:9: error: expected ';' before 'return'\nsol.cpp:8:5: warning: unused variable 'x'\n";
        let diags = parse_compiler_diagnostics(gcc, "sol.cpp");
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0].line, 13);
        assert_eq!(diags[0].severity, DiagSeverity::Error);
        assert_eq!(diags[1].line, 7);
        assert_eq!(diags[1].severity, DiagSeverity::Warning);
    }

    #[test]
    fn splits_multi_case_in_txt_and_compares_outputs() {
        let dir = std::env::temp_dir().join(format!("nave_judge_cases_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("in.txt"), "1 2\n---\n10 20\n").unwrap();
        fs::write(dir.join("expected.txt"), "3  \n---\n30\n\n").unwrap();

        let cases = discover_test_cases(&dir);
        assert_eq!(cases.len(), 2);
        assert!(cases[0].label.contains("in.txt ↔ expected.txt"));
        assert!(outputs_match("3\n", cases[0].expected.as_deref().unwrap()));
        assert!(outputs_match("30", cases[1].expected.as_deref().unwrap()));
        assert!(!outputs_match("31", cases[1].expected.as_deref().unwrap()));
        assert_eq!(first_diff_line("3\n31\n", "3\n30\n"), Some(2));

        // Empty expected.txt must NOT trigger a false WA against empty string or stale out.txt
        fs::write(dir.join("expected.txt"), "   \n").unwrap();
        fs::write(dir.join("out.txt"), "stale\n").unwrap();
        let cases_empty_exp = discover_test_cases(&dir);
        assert!(cases_empty_exp[0].expected.is_none());

        // Relative bare filename `sol.cpp` must NEVER resolve to empty dir ""
        let resolved = resolve_src_dir(Path::new("sol.cpp"));
        assert!(!resolved.as_os_str().is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
