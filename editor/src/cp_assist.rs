use crate::rope::Rope;
use crate::syntax::Language;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

// =============================================================================
// 1. Smart Identifier, STL & Snippet Autocomplete Engine (<0.2ms, zero LSP)
// =============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionKind {
    DeclaredVar,
    Identifier,
    Stl,
    Snippet,
}

impl CompletionKind {
    pub fn tag(self) -> &'static str {
        match self {
            CompletionKind::DeclaredVar => "var",
            CompletionKind::Identifier => "id",
            CompletionKind::Stl => "stl",
            CompletionKind::Snippet => "snip",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompletionItem {
    pub label: String,
    pub insert: String,
    pub detail: String,
    pub kind: CompletionKind,
    pub score: i32,
}

#[derive(Clone, Debug)]
pub struct CompletionPopup {
    pub prefix: String,
    pub start_byte: usize,
    pub items: Vec<CompletionItem>,
    pub selected: usize,
    pub user_navigated: bool,
}

impl CompletionPopup {
    pub fn active(&self) -> Option<&CompletionItem> {
        self.items.get(self.selected)
    }

    pub fn ghost_suffix(&self) -> Option<&str> {
        let item = self.active()?;
        if item.kind == CompletionKind::Snippet {
            return None;
        }
        item.label.strip_prefix(&self.prefix).filter(|s| !s.is_empty())
    }

    pub fn move_sel(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let n = self.items.len() as isize;
        self.selected = ((self.selected as isize + delta).rem_euclid(n)) as usize;
        self.user_navigated = true;
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

/// Computes ranked completions at `cursor_byte` in `rope`.
/// - When `forced == false`: requires prefix >= 2 chars, candidate >= `min_ident_len` (default 4),
///   and candidate saves at least 2 extra chars (`cand.len() >= prefix.len() + 2`), EXCEPT exact
///   member-access (`.`, `->`, `::`) or exact snippet trigger when `!exam_mode`.
/// - When `forced == true` (`Ctrl+Space`): triggers from prefix >= 1 char (or after `.`/`->`/`::`).
pub fn compute_completions(
    rope: &Rope,
    cursor_byte: usize,
    lang: Language,
    min_ident_len: usize,
    exam_mode: bool,
    forced: bool,
) -> Option<CompletionPopup> {
    let (row, col) = rope.byte_to_line_col(cursor_byte);
    let line = rope.line(row);
    let line_start = rope.line_start_byte(row);

    // Byte offset inside `line`
    let col_byte = line
        .char_indices()
        .nth(col)
        .map(|(b, _)| b)
        .unwrap_or(line.len());

    // Do not auto-trigger in the middle of an identifier
    if !forced && col_byte < line.len() && is_ident_byte(line.as_bytes()[col_byte]) {
        return None;
    }

    let before = &line[..col_byte];
    // Skip inside obvious line comments
    let trimmed_before = before.trim_start();
    if trimmed_before.starts_with("//") || (lang == Language::Python && trimmed_before.starts_with('#')) {
        return None;
    }

    // Extract identifier prefix immediately before cursor
    let bytes = before.as_bytes();
    let mut pref_start = bytes.len();
    while pref_start > 0 && is_ident_byte(bytes[pref_start - 1]) {
        pref_start -= 1;
    }
    let prefix = &before[pref_start..];
    if !prefix.is_empty() && !is_ident_start(prefix.as_bytes()[0]) {
        return None;
    }

    let after_member = before[..pref_start].ends_with('.')
        || before[..pref_start].ends_with("->")
        || before[..pref_start].ends_with("::");

    if !forced {
        if after_member {
            if prefix.len() < 2 {
                return None;
            }
        } else if prefix.len() < 2 {
            return None;
        }
    } else if prefix.is_empty() && !after_member {
        return None;
    }

    let eff_min_len = if forced { 2 } else { min_ident_len.max(3) };
    let start_byte = line_start + pref_start;
    let pref_lower = prefix.to_ascii_lowercase();

    let mut best_map: HashMap<String, CompletionItem> = HashMap::new();
    let mut insert_cand = |item: CompletionItem| {
        if let Some(existing) = best_map.get_mut(&item.label) {
            if item.score > existing.score {
                *existing = item;
            }
        } else {
            best_map.insert(item.label.clone(), item);
        }
    };

    // 1. Scan document identifiers & declarations (capped to 4000 lines around cursor for <0.2ms speed)
    if !after_member {
        let total_lines = rope.line_count();
        let scan_start = row.saturating_sub(2000);
        let scan_end = (row + 2000).min(total_lines);

        let mut freq: HashMap<String, (usize, usize, bool)> = HashMap::new(); // (count, closest_dist, is_decl)
        for r in scan_start..scan_end {
            let ltxt = rope.line(r);
            let lbytes = ltxt.as_bytes();
            let dist = r.abs_diff(row);
            let is_above = r <= row;
            let trimmed = ltxt.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
                continue;
            }

            let mut i = 0usize;
            let mut prev_word = "";
            let mut in_str: Option<u8> = None;
            while i < lbytes.len() {
                let b = lbytes[i];
                if let Some(q) = in_str {
                    if b == b'\\' {
                        i += 2;
                        continue;
                    }
                    if b == q {
                        in_str = None;
                    }
                    i += 1;
                    continue;
                }
                if b == b'"' || b == b'\'' {
                    in_str = Some(b);
                    i += 1;
                    continue;
                }
                if b == b'/' && i + 1 < lbytes.len() && lbytes[i + 1] == b'/' {
                    break;
                }
                if lang == Language::Python && b == b'#' {
                    break;
                }

                if is_ident_start(b) {
                    let s = i;
                    while i < lbytes.len() && is_ident_byte(lbytes[i]) {
                        i += 1;
                    }
                    // Skip the exact token currently being typed at the cursor
                    if r == row && s == pref_start && i == col_byte {
                        continue;
                    }
                    let word = &ltxt[s..i];
                    if word.len() < eff_min_len || word == prefix {
                        prev_word = word;
                        continue;
                    }
                    if !forced && word.len() < prefix.len() + 2 {
                        prev_word = word;
                        continue;
                    }
                    let exact_pref = word.starts_with(prefix);
                    let ci_pref = exact_pref || word.to_ascii_lowercase().starts_with(&pref_lower);
                    if !ci_pref {
                        prev_word = word;
                        continue;
                    }

                    // Detect if this looks like a variable/type/fn/macro declaration
                    let mut rest_after = &ltxt[i..];
                    rest_after = rest_after.trim_start();
                    let is_decl = is_decl_lead(prev_word, lang)
                        || (lang == Language::Python && rest_after.starts_with('=') && !rest_after.starts_with("=="))
                        || (matches!(lang, Language::CLike | Language::Rust | Language::Java | Language::Go)
                            && (rest_after.starts_with('=')
                                || rest_after.starts_with('(')
                                || rest_after.starts_with(';')
                                || rest_after.starts_with('[')
                                || rest_after.starts_with(','))
                            && !prev_word.is_empty()
                            && is_type_like(prev_word));

                    let entry = freq.entry(word.to_string()).or_insert((0, dist, false));
                    entry.0 += 1;
                    if dist < entry.1 || (is_above && dist <= entry.1 + 5) {
                        entry.1 = dist;
                    }
                    if is_decl {
                        entry.2 = true;
                    }
                    prev_word = word;
                } else {
                    if !b.is_ascii_whitespace() && !matches!(b, b'&' | b'*' | b'>' | b',') {
                        prev_word = "";
                    }
                    i += 1;
                }
            }
        }

        for (word, (count, dist, is_decl)) in freq {
            let exact_case = word.starts_with(prefix);
            let mut score = if is_decl { 320 } else { 180 };
            if exact_case {
                score += 60;
            }
            score += (50i32 - (dist as i32).min(50)) * 2;
            score += (count as i32).min(15) * 4;
            score -= (word.len() as i32) / 3;
            insert_cand(CompletionItem {
                label: word.clone(),
                insert: word,
                detail: if is_decl { "declarada".into() } else { "buffer".into() },
                kind: if is_decl {
                    CompletionKind::DeclaredVar
                } else {
                    CompletionKind::Identifier
                },
                score,
            });
        }
    }

    // 2. Curated STL / Language Builtins & Member Methods (zero-latency CP dictionary)
    let dict: &[(&str, &str)] = if after_member {
        match lang {
            Language::CLike => CPP_MEMBER_STL,
            Language::Python => PY_MEMBER_STL,
            Language::Rust => RUST_MEMBER_STL,
            _ => &[],
        }
    } else {
        match lang {
            Language::CLike => CPP_CP_STL,
            Language::Python => PY_CP_STL,
            Language::Rust => RUST_CP_STL,
            _ => &[],
        }
    };

    let dict_len = dict.len();
    for (idx, &(sym, desc)) in dict.iter().enumerate() {
        if sym == prefix {
            continue;
        }
        if !after_member && sym.len() < eff_min_len {
            continue;
        }
        if !forced && !after_member && sym.len() < prefix.len() + 2 {
            continue;
        }
        if !prefix.is_empty() && !sym.to_ascii_lowercase().starts_with(&pref_lower) {
            continue;
        }
        let exact = sym.starts_with(prefix);
        let prio = ((dict_len - idx) * 35 / dict_len.max(1)) as i32;
        let score = if after_member { 260 } else { 110 } + if exact { 40 } else { 0 } + prio;
        insert_cand(CompletionItem {
            label: sym.to_string(),
            insert: sym.to_string(),
            detail: desc.to_string(),
            kind: CompletionKind::Stl,
            score,
        });
    }

    // 3. Offline CP Snippets (disabled automatically in Exam Mode!)
    if !exam_mode && !after_member && !prefix.is_empty() {
        for snip in all_snippets(lang) {
            if snip.trigger.starts_with(prefix) {
                let exact = snip.trigger == prefix;
                if exact || forced || snip.trigger.len() >= prefix.len() + 1 {
                    insert_cand(CompletionItem {
                        label: format!("{} ▸", snip.trigger),
                        insert: snip.body,
                        detail: snip.description,
                        kind: CompletionKind::Snippet,
                        score: if exact { 420 } else { 140 },
                    });
                }
            }
        }
    }

    let mut items: Vec<CompletionItem> = best_map.into_values().collect();
    if items.is_empty() {
        return None;
    }
    items.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.label.cmp(&b.label)));
    items.truncate(8);

    Some(CompletionPopup {
        prefix: prefix.to_string(),
        start_byte,
        items,
        selected: 0,
        user_navigated: false,
    })
}

fn is_decl_lead(word: &str, lang: Language) -> bool {
    match lang {
        Language::CLike => matches!(
            word,
            "int" | "long" | "ll" | "ull" | "i64" | "u64" | "double" | "ld" | "float" | "bool"
                | "char" | "auto" | "string" | "str" | "size_t" | "uint32_t" | "uint64_t"
                | "int64_t" | "int32_t" | "void" | "const" | "constexpr" | "static" | "struct"
                | "class" | "enum" | "using" | "typedef" | "define" | "vi" | "vll" | "pii" | "pll"
                | "vector" | "pair" | "map" | "set" | "unordered_map" | "unordered_set"
                | "priority_queue" | "queue" | "stack" | "deque" | "array" | "bitset"
        ),
        Language::Python => matches!(word, "def" | "class" | "for" | "as" | "import" | "global" | "nonlocal"),
        Language::Rust => matches!(word, "let" | "mut" | "const" | "static" | "fn" | "struct" | "enum" | "type" | "for"),
        Language::JsTs => matches!(word, "const" | "let" | "var" | "function" | "class" | "type" | "interface"),
        Language::Go => matches!(word, "var" | "const" | "func" | "type"),
        Language::Java => matches!(word, "int" | "long" | "double" | "boolean" | "char" | "String" | "var" | "final" | "class"),
        _ => false,
    }
}

fn is_type_like(word: &str) -> bool {
    word.as_bytes().first().is_some_and(|b| b.is_ascii_uppercase())
        || matches!(
            word,
            "int" | "long" | "ll" | "ull" | "double" | "ld" | "float" | "bool" | "char"
                | "auto" | "string" | "vi" | "vll" | "pii" | "pll" | "usize" | "isize"
                | "i32" | "i64" | "u32" | "u64" | "f64"
        )
}

const CPP_CP_STL: &[(&str, &str)] = &[
    ("vector", "STL"),
    ("priority_queue", "STL heap"),
    ("unordered_map", "STL hash"),
    ("unordered_set", "STL hash"),
    ("multiset", "STL"),
    ("multimap", "STL"),
    ("deque", "STL"),
    ("queue", "STL"),
    ("stack", "STL"),
    ("bitset", "STL"),
    ("pair", "STL"),
    ("tuple", "STL"),
    ("string", "STL"),
    ("lower_bound", "algo"),
    ("upper_bound", "algo"),
    ("binary_search", "algo"),
    ("next_permutation", "algo"),
    ("prev_permutation", "algo"),
    ("reverse", "algo"),
    ("unique", "algo"),
    ("accumulate", "algo"),
    ("partial_sum", "algo"),
    ("iota", "algo"),
    ("nth_element", "algo"),
    ("stable_sort", "algo"),
    ("min_element", "algo"),
    ("max_element", "algo"),
    ("make_pair", "STL"),
    ("make_tuple", "STL"),
    ("to_string", "STL"),
    ("stoll", "STL"),
    ("getline", "IO"),
    ("fixed", "IO"),
    ("setprecision", "IO"),
    ("INT_MAX", "const"),
    ("INT_MIN", "const"),
    ("LLONG_MAX", "const"),
    ("LLONG_MIN", "const"),
    ("__builtin_popcount", "bit"),
    ("__builtin_popcountll", "bit"),
    ("__builtin_clzll", "bit"),
    ("__builtin_ctzll", "bit"),
    ("__gcd", "math"),
    ("numeric_limits", "STL"),
];

const CPP_MEMBER_STL: &[(&str, &str)] = &[
    ("push_back", "vec"),
    ("emplace_back", "vec"),
    ("pop_back", "vec"),
    ("begin", "iter"),
    ("end", "iter"),
    ("rbegin", "iter"),
    ("rend", "iter"),
    ("size", "len"),
    ("empty", "bool"),
    ("clear", "mut"),
    ("resize", "mut"),
    ("reserve", "cap"),
    ("assign", "mut"),
    ("front", "elem"),
    ("back", "elem"),
    ("push", "pq/q"),
    ("emplace", "pq/set"),
    ("pop", "pq/q"),
    ("top", "pq/stk"),
    ("insert", "set/map"),
    ("erase", "mut"),
    ("find", "lookup"),
    ("count", "lookup"),
    ("contains", "C++20"),
    ("lower_bound", "ordered"),
    ("upper_bound", "ordered"),
    ("substr", "string"),
    ("first", "pair"),
    ("second", "pair"),
    ("push_front", "deque"),
    ("pop_front", "deque"),
];

const PY_CP_STL: &[(&str, &str)] = &[
    ("defaultdict", "collections"),
    ("Counter", "collections"),
    ("deque", "collections"),
    ("bisect_left", "bisect"),
    ("bisect_right", "bisect"),
    ("heappush", "heapq"),
    ("heappop", "heapq"),
    ("heapify", "heapq"),
    ("lru_cache", "functools"),
    ("cache", "functools"),
    ("accumulate", "itertools"),
    ("combinations", "itertools"),
    ("permutations", "itertools"),
    ("enumerate", "builtin"),
    ("sorted", "builtin"),
    ("reversed", "builtin"),
    ("readline", "sys.stdin"),
    ("setrecursionlimit", "sys"),
    ("math.gcd", "math"),
    ("math.lcm", "math"),
    ("math.isqrt", "math"),
    ("math.comb", "math"),
    ("float('inf')", "const"),
];

const PY_MEMBER_STL: &[(&str, &str)] = &[
    ("append", "list"),
    ("appendleft", "deque"),
    ("popleft", "deque"),
    ("extend", "list"),
    ("insert", "list"),
    ("remove", "list"),
    ("reverse", "list"),
    ("sort", "list"),
    ("index", "seq"),
    ("count", "seq"),
    ("split", "str"),
    ("strip", "str"),
    ("join", "str"),
    ("startswith", "str"),
    ("endswith", "str"),
    ("items", "dict"),
    ("keys", "dict"),
    ("values", "dict"),
    ("setdefault", "dict"),
    ("update", "dict/set"),
    ("discard", "set"),
];

const RUST_CP_STL: &[(&str, &str)] = &[
    ("VecDeque", "std"),
    ("BinaryHeap", "std"),
    ("HashMap", "std"),
    ("HashSet", "std"),
    ("BTreeMap", "std"),
    ("BTreeSet", "std"),
    ("Reverse", "std::cmp"),
    ("println!", "macro"),
    ("eprintln!", "macro"),
    ("format!", "macro"),
    ("vec!", "macro"),
];

const RUST_MEMBER_STL: &[(&str, &str)] = &[
    ("push", "vec"),
    ("pop", "vec"),
    ("push_back", "deque"),
    ("pop_front", "deque"),
    ("iter", "iter"),
    ("into_iter", "iter"),
    ("enumerate", "iter"),
    ("collect", "iter"),
    ("unwrap_or", "opt"),
    ("unwrap_or_default", "opt"),
    ("entry", "map"),
    ("or_insert", "map"),
    ("binary_search", "slice"),
    ("partition_point", "slice"),
    ("sort_unstable", "slice"),
    ("windows", "slice"),
];

// =============================================================================
// 2. Offline CP Snippets Library (Built-ins + ~/.config/nave/snippets/*)
// =============================================================================

#[derive(Clone, Debug)]
pub struct Snippet {
    pub trigger: String,
    pub description: String,
    pub body: String,
}

pub fn snippets_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".config")
        })
        .join("nave/snippets")
}

pub fn all_snippets(lang: Language) -> Vec<Snippet> {
    let mut map: HashMap<String, Snippet> = HashMap::new();

    if lang == Language::Python {
        for &(trig, desc, body) in PY_SNIPPETS {
            map.insert(
                trig.to_string(),
                Snippet {
                    trigger: trig.to_string(),
                    description: desc.to_string(),
                    body: body.to_string(),
                },
            );
        }
    } else {
        for &(trig, desc, body) in CPP_SNIPPETS {
            map.insert(
                trig.to_string(),
                Snippet {
                    trigger: trig.to_string(),
                    description: desc.to_string(),
                    body: body.to_string(),
                },
            );
        }
    }

    // Load custom user snippets from ~/.config/nave/snippets/<trigger>.{txt,cpp,py}
    let dir = snippets_dir();
    if let Ok(entries) = fs::read_dir(&dir) {
        for ent in entries.flatten() {
            let p = ent.path();
            if !p.is_file() {
                continue;
            }
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            let matches_lang = match ext {
                "py" => lang == Language::Python,
                "cpp" | "cc" | "h" => lang == Language::CLike,
                "txt" | "snip" | "" => true,
                _ => false,
            };
            if !matches_lang {
                continue;
            }
            let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else { continue };
            if let Ok(body) = fs::read_to_string(&p) {
                if !body.is_empty() {
                    map.insert(
                        stem.to_string(),
                        Snippet {
                            trigger: stem.to_string(),
                            description: "custom snippet".to_string(),
                            body: body.trim_end_matches('\n').to_string(),
                        },
                    );
                }
            }
        }
    }

    let mut out: Vec<Snippet> = map.into_values().collect();
    out.sort_by(|a, b| a.trigger.cmp(&b.trigger));
    out
}

pub fn find_snippet(trigger: &str, lang: Language) -> Option<Snippet> {
    all_snippets(lang).into_iter().find(|s| s.trigger == trigger)
}

const CPP_SNIPPETS: &[(&str, &str, &str)] = &[
    (
        "cp",
        "Template Codeforces (multi-test + fast IO)",
        "#include <bits/stdc++.h>\nusing namespace std;\n\nusing ll = long long;\nusing pii = pair<int, int>;\nusing pll = pair<ll, ll>;\nusing vi = vector<int>;\nusing vll = vector<ll>;\n\n#define all(x) (x).begin(), (x).end()\n#define sz(x) (int)(x).size()\n\n#ifdef LOCAL\n#define dbg(x) cerr << \"[\" << #x << \" = \" << (x) << \"]\\n\"\n#else\n#define dbg(x)\n#endif\n\nvoid solve() {\n    int n;\n    cin >> n;\n    \n}\n\nint main() {\n    ios::sync_with_stdio(false);\n    cin.tie(nullptr);\n    int t = 1;\n    cin >> t;\n    while (t--) solve();\n    return 0;\n}",
    ),
    (
        "cp1",
        "Template ICPC / single-test (fast IO)",
        "#include <bits/stdc++.h>\nusing namespace std;\n\nusing ll = long long;\nusing vi = vector<int>;\nusing vll = vector<ll>;\n#define all(x) (x).begin(), (x).end()\n\nint main() {\n    ios::sync_with_stdio(false);\n    cin.tie(nullptr);\n    int n;\n    if (!(cin >> n)) return 0;\n    \n    return 0;\n}",
    ),
    (
        "dsu",
        "Disjoint Set Union (path compression + size)",
        "struct DSU {\n    vector<int> p, sz;\n    DSU(int n) : p(n), sz(n, 1) { iota(p.begin(), p.end(), 0); }\n    int find(int x) { return p[x] == x ? x : p[x] = find(p[x]); }\n    bool unite(int a, int b) {\n        a = find(a); b = find(b);\n        if (a == b) return false;\n        if (sz[a] < sz[b]) swap(a, b);\n        p[b] = a; sz[a] += sz[b];\n        return true;\n    }\n};",
    ),
    (
        "segtree",
        "Segment Tree iterativa (point update + range sum)",
        "struct SegTree {\n    int n;\n    vector<long long> t;\n    SegTree(int n) : n(n), t(2 * n, 0) {}\n    void update(int p, long long v) {\n        for (t[p += n] = v; p > 1; p >>= 1) t[p >> 1] = t[p] + t[p ^ 1];\n    }\n    long long query(int l, int r) { // [l, r]\n        long long res = 0;\n        for (l += n, r += n + 1; l < r; l >>= 1, r >>= 1) {\n            if (l & 1) res += t[l++];\n            if (r & 1) res += t[--r];\n        }\n        return res;\n    }\n};",
    ),
    (
        "bit",
        "Fenwick Tree / BIT (1-indexed)",
        "struct BIT {\n    int n;\n    vector<long long> bit;\n    BIT(int n) : n(n), bit(n + 1, 0) {}\n    void add(int i, long long delta) {\n        for (; i <= n; i += i & -i) bit[i] += delta;\n    }\n    long long sum(int i) {\n        long long s = 0;\n        for (; i > 0; i -= i & -i) s += bit[i];\n        return s;\n    }\n    long long query(int l, int r) { return sum(r) - sum(l - 1); }\n};",
    ),
    (
        "dijk",
        "Dijkstra shortest path (min-heap)",
        "const long long INF = 4e18;\nvector<long long> dist(n, INF);\npriority_queue<pair<long long, int>, vector<pair<long long, int>>, greater<>> pq;\ndist[src] = 0;\npq.push({0, src});\nwhile (!pq.empty()) {\n    auto [d, u] = pq.top(); pq.pop();\n    if (d != dist[u]) continue;\n    for (auto [v, w] : adj[u]) {\n        if (d + w < dist[v]) {\n            dist[v] = d + w;\n            pq.push({dist[v], v});\n        }\n    }\n}",
    ),
    (
        "bfs",
        "BFS grafo / distancias",
        "vector<int> dist(n, -1);\nqueue<int> q;\ndist[src] = 0;\nq.push(src);\nwhile (!q.empty()) {\n    int u = q.front(); q.pop();\n    for (int v : adj[u]) {\n        if (dist[v] == -1) {\n            dist[v] = dist[u] + 1;\n            q.push(v);\n        }\n    }\n}",
    ),
    (
        "dfs",
        "DFS recursivo com lambda",
        "vector<bool> vis(n, false);\nauto dfs = [&](auto&& self, int u, int p) -> void {\n    vis[u] = true;\n    for (int v : adj[u]) {\n        if (v == p || vis[v]) continue;\n        self(self, v, u);\n    }\n};\ndfs(dfs, 0, -1);",
    ),
    (
        "lca",
        "Binary Lifting LCA",
        "const int LOG = 20;\nvector<vector<int>> up(LOG, vector<int>(n));\nvector<int> depth(n, 0);\nauto dfs_lca = [&](auto&& self, int u, int p) -> void {\n    up[0][u] = p;\n    for (int k = 1; k < LOG; k++) up[k][u] = up[k - 1][up[k - 1][u]];\n    for (int v : adj[u]) if (v != p) {\n        depth[v] = depth[u] + 1;\n        self(self, v, u);\n    }\n};\nauto lca = [&](int u, int v) -> int {\n    if (depth[u] < depth[v]) swap(u, v);\n    int diff = depth[u] - depth[v];\n    for (int k = 0; k < LOG; k++) if ((diff >> k) & 1) u = up[k][u];\n    if (u == v) return u;\n    for (int k = LOG - 1; k >= 0; k--) if (up[k][u] != up[k][v]) { u = up[k][u]; v = up[k][v]; }\n    return up[0][u];\n};",
    ),
    (
        "kmp",
        "Prefix function (KMP)",
        "vector<int> prefix_function(const string& s) {\n    int n = (int)s.size();\n    vector<int> pi(n, 0);\n    for (int i = 1; i < n; i++) {\n        int j = pi[i - 1];\n        while (j > 0 && s[i] != s[j]) j = pi[j - 1];\n        if (s[i] == s[j]) j++;\n        pi[i] = j;\n    }\n    return pi;\n}",
    ),
    (
        "modpow",
        "Exponenciacao modular rapida",
        "const long long MOD = 1e9 + 7;\nlong long modpow(long long a, long long e, long long m = MOD) {\n    long long res = 1;\n    a %= m;\n    while (e > 0) {\n        if (e & 1) res = (__int128)res * a % m;\n        a = (__int128)a * a % m;\n        e >>= 1;\n    }\n    return res;\n}",
    ),
    (
        "sieve",
        "Crivo de Eratostenes",
        "vector<bool> is_prime(MAXN + 1, true);\nis_prime[0] = is_prime[1] = false;\nfor (int i = 2; 1LL * i * i <= MAXN; i++) {\n    if (is_prime[i]) {\n        for (int j = i * i; j <= MAXN; j += i) is_prime[j] = false;\n    }\n}",
    ),
    (
        "dbg",
        "Macro de debug LOCAL para containers",
        "#ifdef LOCAL\ntemplate <typename T> void _dbg(const T& x) { cerr << x; }\ntemplate <typename A, typename B> void _dbg(const pair<A, B>& p) { cerr << '('; _dbg(p.first); cerr << ','; _dbg(p.second); cerr << ')'; }\ntemplate <typename T> void _dbg(const vector<T>& v) { cerr << '['; for (size_t i = 0; i < v.size(); i++) { if (i) cerr << ','; _dbg(v[i]); } cerr << ']'; }\n#define dbg(x) do { cerr << #x << \" = \"; _dbg(x); cerr << '\\n'; } while (0)\n#else\n#define dbg(x)\n#endif",
    ),
];

const PY_SNIPPETS: &[(&str, &str, &str)] = &[
    (
        "cp",
        "Template Python CP (fast stdin)",
        "import sys\ninput = sys.stdin.readline\n\ndef solve():\n    n = int(input())\n    \n\ndef main():\n    t = int(input())\n    for _ in range(t):\n        solve()\n\nif __name__ == '__main__':\n    main()",
    ),
    (
        "dsu",
        "Disjoint Set Union (Python)",
        "class DSU:\n    def __init__(self, n):\n        self.p = list(range(n))\n        self.sz = [1] * n\n    def find(self, x):\n        while self.p[x] != x:\n            self.p[x] = self.p[self.p[x]]\n            x = self.p[x]\n        return x\n    def unite(self, a, b):\n        a, b = self.find(a), self.find(b)\n        if a == b:\n            return False\n        if self.sz[a] < self.sz[b]:\n            a, b = b, a\n        self.p[b] = a\n        self.sz[a] += self.sz[b]\n        return True",
    ),
    (
        "dijk",
        "Dijkstra min-heap (Python)",
        "import heapq\nINF = 10**18\ndist = [INF] * n\ndist[src] = 0\npq = [(0, src)]\nwhile pq:\n    d, u = heapq.heappop(pq)\n    if d != dist[u]:\n        continue\n    for v, w in adj[u]:\n        if d + w < dist[v]:\n            dist[v] = d + w\n            heapq.heappush(pq, (dist[v], v))",
    ),
    (
        "bfs",
        "BFS com deque (Python)",
        "from collections import deque\ndist = [-1] * n\ndist[src] = 0\nq = deque([src])\nwhile q:\n    u = q.popleft()\n    for v in adj[u]:\n        if dist[v] == -1:\n            dist[v] = dist[u] + 1\n            q.append(v)",
    ),
];

// =============================================================================
// 3. Go to Symbol Extractor (`Ctrl+Shift+O`)
// =============================================================================

#[derive(Clone, Debug)]
pub struct SymbolEntry {
    pub name: String,
    pub kind: &'static str,
    pub line: usize, // 1-based
}

pub fn extract_symbols(rope: &Rope, lang: Language) -> Vec<SymbolEntry> {
    let mut out = Vec::new();
    let total = rope.line_count().min(10_000);
    for r in 0..total {
        let line = rope.line(r);
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
            continue;
        }
        let line_no = r + 1;
        match lang {
            Language::Python => {
                if let Some(rest) = trimmed.strip_prefix("def ") {
                    if let Some(name) = rest.split('(').next().map(str::trim).filter(|s| !s.is_empty()) {
                        out.push(SymbolEntry { name: name.into(), kind: "fn", line: line_no });
                    }
                } else if let Some(rest) = trimmed.strip_prefix("async def ") {
                    if let Some(name) = rest.split('(').next().map(str::trim).filter(|s| !s.is_empty()) {
                        out.push(SymbolEntry { name: name.into(), kind: "fn", line: line_no });
                    }
                } else if let Some(rest) = trimmed.strip_prefix("class ") {
                    if let Some(name) = rest.split(['(', ':']).next().map(str::trim).filter(|s| !s.is_empty()) {
                        out.push(SymbolEntry { name: name.into(), kind: "class", line: line_no });
                    }
                }
            }
            Language::Rust => {
                let s = trimmed
                    .strip_prefix("pub(crate) ")
                    .or_else(|| trimmed.strip_prefix("pub "))
                    .unwrap_or(trimmed);
                let s = s.strip_prefix("async ").unwrap_or(s);
                if let Some(rest) = s.strip_prefix("fn ") {
                    if let Some(name) = rest.split(['(', '<']).next().map(str::trim).filter(|n| !n.is_empty()) {
                        out.push(SymbolEntry { name: name.into(), kind: "fn", line: line_no });
                    }
                } else if let Some(rest) = s.strip_prefix("struct ") {
                    if let Some(name) = rest.split(['{', '<', '(', ';']).next().map(str::trim).filter(|n| !n.is_empty()) {
                        out.push(SymbolEntry { name: name.into(), kind: "struct", line: line_no });
                    }
                } else if let Some(rest) = s.strip_prefix("enum ") {
                    if let Some(name) = rest.split(['{', '<']).next().map(str::trim).filter(|n| !n.is_empty()) {
                        out.push(SymbolEntry { name: name.into(), kind: "enum", line: line_no });
                    }
                } else if let Some(rest) = s.strip_prefix("impl ") {
                    let sig = rest.split('{').next().unwrap_or(rest).trim();
                    if !sig.is_empty() {
                        out.push(SymbolEntry { name: sig.into(), kind: "impl", line: line_no });
                    }
                }
            }
            Language::CLike | Language::Java => {
                if let Some(rest) = trimmed.strip_prefix("#define ") {
                    if let Some(name) = rest.split(['(', ' ', '\t']).next().filter(|s| !s.is_empty()) {
                        out.push(SymbolEntry { name: name.into(), kind: "macro", line: line_no });
                    }
                } else if let Some(rest) = trimmed.strip_prefix("struct ").or_else(|| trimmed.strip_prefix("class ")) {
                    let kind = if trimmed.starts_with("struct") { "struct" } else { "class" };
                    if let Some(name) = rest.split(['{', ':', ' ', ';']).next().map(str::trim).filter(|s| !s.is_empty()) {
                        out.push(SymbolEntry { name: name.into(), kind, line: line_no });
                    }
                } else if trimmed.contains('(')
                    && !trimmed.starts_with("if")
                    && !trimmed.starts_with("for")
                    && !trimmed.starts_with("while")
                    && !trimmed.starts_with("switch")
                    && !trimmed.starts_with("return")
                    && !trimmed.starts_with("else")
                    && !trimmed.ends_with(';')
                    && (trimmed.ends_with('{') || trimmed.ends_with(')') || trimmed.ends_with("const {"))
                {
                    if let Some(before_paren) = trimmed.split('(').next() {
                        let name = before_paren
                            .split_whitespace()
                            .last()
                            .unwrap_or("")
                            .trim_start_matches(['*', '&']);
                        if !name.is_empty()
                            && is_ident_start(name.as_bytes()[0])
                            && !matches!(name, "if" | "for" | "while" | "switch" | "catch" | "auto")
                        {
                            out.push(SymbolEntry { name: name.into(), kind: "fn", line: line_no });
                        }
                    }
                }
            }
            Language::Go => {
                if let Some(rest) = trimmed.strip_prefix("func ") {
                    let clean = if rest.starts_with('(') {
                        rest.split_once(')').map(|(_, b)| b.trim()).unwrap_or(rest)
                    } else {
                        rest
                    };
                    if let Some(name) = clean.split(['(', '[']).next().map(str::trim).filter(|s| !s.is_empty()) {
                        out.push(SymbolEntry { name: name.into(), kind: "fn", line: line_no });
                    }
                } else if let Some(rest) = trimmed.strip_prefix("type ") {
                    if let Some(name) = rest.split_whitespace().next() {
                        out.push(SymbolEntry { name: name.into(), kind: "type", line: line_no });
                    }
                }
            }
            Language::JsTs => {
                let s = trimmed.strip_prefix("export ").unwrap_or(trimmed);
                let s = s.strip_prefix("async ").unwrap_or(s);
                if let Some(rest) = s.strip_prefix("function ") {
                    if let Some(name) = rest.split(['(', '<']).next().map(str::trim).filter(|n| !n.is_empty()) {
                        out.push(SymbolEntry { name: name.into(), kind: "fn", line: line_no });
                    }
                } else if let Some(rest) = s.strip_prefix("class ") {
                    if let Some(name) = rest.split(['{', ' ', '<']).next().map(str::trim).filter(|n| !n.is_empty()) {
                        out.push(SymbolEntry { name: name.into(), kind: "class", line: line_no });
                    }
                }
            }
            _ => {}
        }
    }
    out
}

// =============================================================================
// 4. Zero-Latency CP Pitfall Radar (0ms per-line checks)
// =============================================================================

pub fn detect_cp_pitfall(line: &str, lang: Language, file_has_custom_hash: bool) -> Option<&'static str> {
    if lang != Language::CLike {
        return None;
    }
    let code = strip_line_comment_and_strings(line);
    let trimmed = code.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    if contains_word(&code, "endl") {
        return Some("⚠ CP: 'endl' faz flush (use '\\n' p/ evitar TLE)");
    }
    if !file_has_custom_hash
        && (contains_word(&code, "unordered_map") || contains_word(&code, "unordered_set"))
    {
        return Some("⚠ CP: unordered_map e hackeavel no CF (use custom hash ou map)");
    }
    if has_int_shift_overflow(&code) {
        return Some("⚠ CP: '1 << k' (k>=31) da overflow em int (use '1LL << k')");
    }
    if has_assign_in_condition(&code) {
        return Some("⚠ CP: atribuicao '=' em condicao if/while (quis dizer '=='?)");
    }
    None
}

pub fn file_has_custom_hash(rope: &Rope) -> bool {
    let lim = rope.line_count().min(400);
    for r in 0..lim {
        let l = rope.line(r);
        if l.contains("splitmix64") || l.contains("custom_hash") || l.contains("struct chash") {
            return true;
        }
    }
    false
}

fn strip_line_comment_and_strings(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let b = line.as_bytes();
    let mut i = 0;
    let mut in_q: Option<u8> = None;
    while i < b.len() {
        if let Some(q) = in_q {
            if b[i] == b'\\' {
                i += 2;
                continue;
            }
            if b[i] == q {
                in_q = None;
            }
            out.push(' ');
            i += 1;
            continue;
        }
        if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            break;
        }
        if b[i] == b'"' || b[i] == b'\'' {
            in_q = Some(b[i]);
            out.push(' ');
            i += 1;
            continue;
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

fn contains_word(s: &str, word: &str) -> bool {
    let bytes = s.as_bytes();
    let wlen = word.len();
    let mut off = 0;
    while let Some(rel) = s[off..].find(word) {
        let idx = off + rel;
        let left_ok = idx == 0 || !is_ident_byte(bytes[idx - 1]);
        let right_ok = idx + wlen == bytes.len() || !is_ident_byte(bytes[idx + wlen]);
        if left_ok && right_ok {
            return true;
        }
        off = idx + wlen;
    }
    false
}

fn has_int_shift_overflow(code: &str) -> bool {
    let mut off = 0;
    while let Some(rel) = code[off..].find("<<") {
        let pos = off + rel;
        let left = code[..pos].trim_end();
        let right = code[pos + 2..].trim_start();
        let is_plain_one = left.ends_with('1')
            && !left.ends_with("LL")
            && !left.ends_with("ll")
            && left
                .as_bytes()
                .len()
                .checked_sub(2)
                .map_or(true, |idx| !is_ident_byte(left.as_bytes()[idx]));
        if is_plain_one {
            let digits: String = right.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(shift) = digits.parse::<usize>() {
                if shift >= 31 {
                    return true;
                }
            }
        }
        off = pos + 2;
    }
    false
}

fn has_assign_in_condition(code: &str) -> bool {
    for kw in ["if", "while"] {
        let mut off = 0;
        while let Some(rel) = code[off..].find(kw) {
            let idx = off + rel;
            let left_ok = idx == 0 || !is_ident_byte(code.as_bytes()[idx - 1]);
            let after_kw = idx + kw.len();
            let right_ok = after_kw < code.len() && !is_ident_byte(code.as_bytes()[after_kw]);
            if left_ok && right_ok {
                let rest = code[after_kw..].trim_start();
                if let Some(after_open) = rest.strip_prefix('(') {
                    // Extract balanced condition inside (...)
                    let mut depth = 1usize;
                    let mut end = None;
                    for (ci, ch) in after_open.char_indices() {
                        match ch {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                if depth == 0 {
                                    end = Some(ci);
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    if let Some(e) = end {
                        let cond = &after_open[..e];
                        // Ignore C++17 `if (auto x = ...; ...)` or `if (int x = ...)`
                        let ctrim = cond.trim_start();
                        let is_init_decl = ctrim.contains(';')
                            || ctrim.starts_with("auto ")
                            || ctrim.starts_with("int ")
                            || ctrim.starts_with("long ")
                            || ctrim.starts_with("bool ")
                            || ctrim.starts_with("char ");
                        if !is_init_decl {
                            let cb = cond.as_bytes();
                            for i in 0..cb.len() {
                                if cb[i] == b'=' {
                                    let prev = if i > 0 { cb[i - 1] } else { b' ' };
                                    let next = if i + 1 < cb.len() { cb[i + 1] } else { b' ' };
                                    if !matches!(prev, b'=' | b'!' | b'<' | b'>' | b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^')
                                        && next != b'='
                                    {
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            off = after_kw;
        }
    }
    false
}

// =============================================================================
// 5. LeetCode Clean `Solution` Extractor
// =============================================================================

pub fn extract_leetcode_solution(rope: &Rope, lang: Language) -> Option<String> {
    let total = rope.line_count();
    let start_row = (0..total).find(|&r| {
        let t = rope.line(r);
        let tr = t.trim_start();
        tr.starts_with("class Solution") || tr.starts_with("struct Solution")
    })?;

    let mut lines = Vec::new();
    if lang == Language::Python {
        let base_indent = rope.line(start_row).chars().take_while(|c| *c == ' ' || *c == '\t').count();
        lines.push(rope.line(start_row));
        for r in (start_row + 1)..total {
            let l = rope.line(r);
            let tr = l.trim();
            if tr.is_empty() {
                lines.push(l);
                continue;
            }
            let ind = l.chars().take_while(|c| *c == ' ' || *c == '\t').count();
            if ind <= base_indent {
                break;
            }
            if tr.starts_with("print(") && tr.contains("# dbg") {
                continue;
            }
            lines.push(l);
        }
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        return Some(lines.join("\n"));
    }

    // Brace-matched extraction for C++ / Java / JS / TS
    let mut depth = 0i32;
    let mut seen_open = false;
    let mut skip_local_ifdef = 0usize;
    for r in start_row..total {
        let l = rope.line(r);
        let tr = l.trim();
        if tr.starts_with("#ifdef LOCAL") {
            skip_local_ifdef += 1;
            continue;
        }
        if skip_local_ifdef > 0 {
            if tr.starts_with("#endif") {
                skip_local_ifdef -= 1;
            }
            continue;
        }
        if tr.starts_with("dbg(") {
            continue;
        }
        lines.push(l.clone());
        let code = strip_line_comment_and_strings(&l);
        for ch in code.chars() {
            if ch == '{' {
                depth += 1;
                seen_open = true;
            } else if ch == '}' {
                depth -= 1;
            }
        }
        if seen_open && depth <= 0 {
            break;
        }
    }
    if seen_open {
        Some(lines.join("\n"))
    } else {
        None
    }
}

// =============================================================================
// 6. CP Practice Timer (Stopwatch & Countdown for LeetCode / Codeforces / ICPC)
// =============================================================================

#[derive(Clone, Debug)]
pub struct CpTimer {
    pub active: bool,
    pub running: bool,
    pub countdown_secs: Option<u64>,
    accumulated: Duration,
    started_at: Option<Instant>,
}

impl Default for CpTimer {
    fn default() -> Self {
        Self {
            active: false,
            running: false,
            countdown_secs: None,
            accumulated: Duration::ZERO,
            started_at: None,
        }
    }
}

impl CpTimer {
    pub fn start_stopwatch(&mut self) {
        self.active = true;
        self.running = true;
        self.countdown_secs = None;
        self.accumulated = Duration::ZERO;
        self.started_at = Some(Instant::now());
    }

    pub fn start_countdown(&mut self, minutes: u64) {
        self.active = true;
        self.running = true;
        self.countdown_secs = Some(minutes * 60);
        self.accumulated = Duration::ZERO;
        self.started_at = Some(Instant::now());
    }

    pub fn toggle_pause(&mut self) {
        if !self.active {
            self.start_stopwatch();
            return;
        }
        if self.running {
            if let Some(st) = self.started_at.take() {
                self.accumulated += st.elapsed();
            }
            self.running = false;
        } else {
            self.started_at = Some(Instant::now());
            self.running = true;
        }
    }

    pub fn stop(&mut self) {
        *self = Self::default();
    }

    pub fn elapsed_secs(&self) -> u64 {
        let extra = self.started_at.map(|s| s.elapsed()).unwrap_or(Duration::ZERO);
        (self.accumulated + extra).as_secs()
    }

    /// Returns `(display_label, is_expired)` for rendering in the bottom status bar.
    pub fn badge(&self) -> Option<(String, bool)> {
        if !self.active {
            return None;
        }
        let elapsed = self.elapsed_secs();
        let pause_tag = if self.running { "" } else { " ⏸" };
        if let Some(target) = self.countdown_secs {
            if elapsed >= target {
                return Some((format!("⏳ 00:00 FIM!{}", pause_tag), true));
            }
            let rem = target - elapsed;
            Some((format!("⏳ {}{}", format_hms(rem), pause_tag), false))
        } else {
            Some((format!("⏱ {}{}", format_hms(elapsed), pause_tag), false))
        }
    }
}

fn format_hms(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autocomplete_ranks_declared_vars_and_respects_min_len() {
        let src = "int main() {\n    int idx = 0;\n    long long answer = 42;\n    vector<int> visited(10);\n    vi\n}\n";
        let rope = Rope::from_str(src);
        let cursor = rope.line_col_to_byte(4, 6); // after "    vi"
        let pop = compute_completions(&rope, cursor, Language::CLike, 4, false, false)
            .expect("expected popup for 'vi'");
        assert_eq!(pop.items[0].label, "visited");
        assert_eq!(pop.items[0].kind, CompletionKind::DeclaredVar);
        assert_eq!(pop.ghost_suffix(), Some("sited"));

        // Short 3-char var `idx` should NOT auto-trigger when min_len=4
        let src2 = "int idx = 0;\nid";
        let rope2 = Rope::from_str(src2);
        let c2 = rope2.line_col_to_byte(1, 2);
        let pop2 = compute_completions(&rope2, c2, Language::CLike, 4, false, false);
        assert!(pop2.map_or(true, |p| !p.items.iter().any(|it| it.label == "idx")));
    }

    #[test]
    fn autocomplete_member_stl_and_exam_mode_blocks_snippets() {
        let src = "vector<int> dp;\ndp.em";
        let rope = Rope::from_str(src);
        let c = rope.line_col_to_byte(1, 5);
        let pop = compute_completions(&rope, c, Language::CLike, 4, false, false).unwrap();
        assert_eq!(pop.items[0].label, "emplace_back");
        assert_eq!(pop.items[0].kind, CompletionKind::Stl);

        // Snippet `dsu` appears normally, blocked in exam_mode
        let r_s = Rope::from_str("dsu");
        let p_norm = compute_completions(&r_s, 3, Language::CLike, 4, false, false).unwrap();
        assert!(p_norm.items.iter().any(|it| it.kind == CompletionKind::Snippet));
        let p_exam = compute_completions(&r_s, 3, Language::CLike, 4, true, false);
        assert!(p_exam.map_or(true, |p| !p.items.iter().any(|it| it.kind == CompletionKind::Snippet)));
    }

    #[test]
    fn pitfall_radar_catches_cp_traps() {
        assert!(detect_cp_pitfall("cout << ans << endl;", Language::CLike, false).is_some());
        assert!(detect_cp_pitfall("int x = 1 << 31;", Language::CLike, false).is_some());
        assert!(detect_cp_pitfall("long long x = 1LL << 31;", Language::CLike, false).is_none());
        assert!(detect_cp_pitfall("unordered_map<int,int> mp;", Language::CLike, false).is_some());
        assert!(detect_cp_pitfall("unordered_map<int,int,custom_hash> mp;", Language::CLike, true).is_none());
        assert!(detect_cp_pitfall("if (a = b) return;", Language::CLike, false).is_some());
        assert!(detect_cp_pitfall("if (a == b) return;", Language::CLike, false).is_none());
    }

    #[test]
    fn extracts_symbols_and_clean_leetcode_solution() {
        let src = "#include <bits/stdc++.h>\nstruct DSU {\n    int n;\n};\nclass Solution {\npublic:\n    int twoSum(vector<int>& a) {\n        dbg(a);\n        return 0;\n    }\n};\nint main() {}\n";
        let rope = Rope::from_str(src);
        let syms = extract_symbols(&rope, Language::CLike);
        assert!(syms.iter().any(|s| s.name == "DSU" && s.kind == "struct"));
        assert!(syms.iter().any(|s| s.name == "Solution" && s.kind == "class"));
        assert!(syms.iter().any(|s| s.name == "twoSum" && s.kind == "fn"));

        let lc = extract_leetcode_solution(&rope, Language::CLike).unwrap();
        assert!(lc.starts_with("class Solution"));
        assert!(!lc.contains("dbg(a)"));
        assert!(!lc.contains("int main()"));
    }
}
