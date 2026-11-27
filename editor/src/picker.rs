use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Debug)]
pub struct PickerItem {
    pub label: String,
    pub detail: String,
    pub action: PickerAction,
}

#[derive(Clone, Debug)]
pub enum PickerAction {
    OpenFile(PathBuf),
    ToggleVimVisual,
    ToggleRelativeNumbers,
    ToggleBlockCursor,
    ToggleAutoHideMouse,
    ToggleWheelMovesCursor,
    ToggleScrollbar,
    ToggleCenterScroll,
    ToggleCursorWord,
    ToggleIndentGuides,
    ToggleRainbowBrackets,
    ToggleBracketMatch,
    ToggleCommentBadges,
    ToggleTrailingSpaces,
    CycleColorColumn,
    ToggleWrap,
    ToggleAutoClose,
    ToggleLineEnding,
    TrimTrailingSpaces,
    ReloadFromDisk,
    GotoLine,
    SaveAs,
    NewBuffer,
    SplitHorizontal,
    SplitVertical,
    ToggleSplitOrientation,
    MoveTabRight,
    MoveTabLeft,
    MoveTabDown,
    MoveTabUp,
    SwapPanes,
    ResizePaneMode,
    EqualizePanes,
    ClosePane,
    SetPalette(usize),
}

#[derive(Clone, Debug)]
pub struct ScoredItem {
    pub index: usize,
    pub score: i32,
    pub matched_chars: Vec<usize>,
}

pub struct Picker {
    pub title: String,
    pub query: String,
    pub items: Vec<PickerItem>,
    pub filtered: Vec<ScoredItem>,
    pub selected: usize,
}

impl Picker {
    pub fn quick_open(base_dir: &Path) -> Self {
        let root = find_project_root(base_dir).unwrap_or_else(|| base_dir.to_path_buf());
        let mut files = Vec::new();
        walk_files(&root, &root, &mut files, 0, 8_000);
        files.sort();
        let items = files
            .into_iter()
            .map(|rel| {
                let full = root.join(&rel);
                PickerItem {
                    label: rel.to_string_lossy().into_owned(),
                    detail: String::new(),
                    action: PickerAction::OpenFile(full),
                }
            })
            .collect();
        let mut p = Self {
            title: format!("Abrir Arquivo ({})", root.file_name().and_then(|s| s.to_str()).unwrap_or(".")),
            query: String::new(),
            items,
            filtered: Vec::new(),
            selected: 0,
        };
        p.refilter();
        p
    }

    pub fn command_palette(cfg: &crate::config::EditorConfig, eol: &str) -> Self {
        let onoff = |b: bool| if b { "ON" } else { "OFF" };
        let items = vec![
            PickerItem {
                label: format!("Estilo Vim Visual [{}]", onoff(cfg.vim_visual)),
                detail: "Alt+V (relnum + scroll cursor + scrolloff)".into(),
                action: PickerAction::ToggleVimVisual,
            },
            PickerItem {
                label: format!("Numeros de Linha Relativos [{}]", onoff(cfg.relative_numbers)),
                detail: "relativenumber".into(),
                action: PickerAction::ToggleRelativeNumbers,
            },
            PickerItem {
                label: format!("Cursor em Bloco Solido [{}]", onoff(cfg.block_cursor)),
                detail: "█ vs |".into(),
                action: PickerAction::ToggleBlockCursor,
            },
            PickerItem {
                label: format!("Auto-Hide Ponteiro do Mouse [{}]", onoff(cfg.auto_hide_mouse)),
                detail: "some ao digitar ou 0.5s ocioso".into(),
                action: PickerAction::ToggleAutoHideMouse,
            },
            PickerItem {
                label: format!("Scroll do Mouse Move o Cursor [{}]", onoff(cfg.wheel_moves_cursor)),
                detail: "ON = move cursor (1 ln) · OFF = rola pagina".into(),
                action: PickerAction::ToggleWheelMovesCursor,
            },
            PickerItem {
                label: format!("Barra de Rolagem Lateral [{}]", onoff(cfg.scrollbar)),
                detail: "┃ borda direita".into(),
                action: PickerAction::ToggleScrollbar,
            },
            PickerItem {
                label: format!("Scroll Centralizado / Typewriter [{}]", onoff(cfg.center_scroll)),
                detail: "mantem linha no centro".into(),
                action: PickerAction::ToggleCenterScroll,
            },
            PickerItem {
                label: format!("Destacar Palavra sob Cursor [{}]", onoff(cfg.cursorword)),
                detail: "illuminate / cursorword".into(),
                action: PickerAction::ToggleCursorWord,
            },
            PickerItem {
                label: format!("Guias de Indentacao [{}]", onoff(cfg.indent_guides)),
                detail: "▏ indent-blankline".into(),
                action: PickerAction::ToggleIndentGuides,
            },
            PickerItem {
                label: format!("Cor de Delimitadores (Rainbow Brackets) [{}]", onoff(cfg.rainbow_brackets)),
                detail: "() [] {} <> por nivel (OFF = neutro)".into(),
                action: PickerAction::ToggleRainbowBrackets,
            },
            PickerItem {
                label: format!("Destaque de Par de Delimitadores (MatchParen) [{}]", onoff(cfg.bracket_match)),
                detail: "fundo em () [] {} ao lado do cursor".into(),
                action: PickerAction::ToggleBracketMatch,
            },
            PickerItem {
                label: format!("Badges TODO / FIXME em Comentarios [{}]", onoff(cfg.comment_badges)),
                detail: "TODO NOTE FIXME BUG HACK".into(),
                action: PickerAction::ToggleCommentBadges,
            },
            PickerItem {
                label: format!("Mostrar Espacos no Fim da Linha [{}]", onoff(cfg.show_trailing)),
                detail: "· listchars trail".into(),
                action: PickerAction::ToggleTrailingSpaces,
            },
            PickerItem {
                label: format!(
                    "Regua de Coluna (colorcolumn) [{}]",
                    if cfg.color_column == 0 { "OFF".into() } else { cfg.color_column.to_string() }
                ),
                detail: "0 / 80 / 100 / 120".into(),
                action: PickerAction::CycleColorColumn,
            },
            PickerItem {
                label: format!("Alternar Word Wrap [{}]", onoff(cfg.wrap)),
                detail: "Alt+Z".into(),
                action: PickerAction::ToggleWrap,
            },
            PickerItem {
                label: format!("Alternar Auto-Close Pares [{}]", onoff(cfg.auto_close)),
                detail: "() [] {} \"\"".into(),
                action: PickerAction::ToggleAutoClose,
            },
            PickerItem {
                label: format!("Converter Final de Linha [atual: {}]", eol),
                detail: "LF <-> CRLF".into(),
                action: PickerAction::ToggleLineEnding,
            },
            PickerItem {
                label: "Remover espacos no fim das linhas (Trim Trailing)".into(),
                detail: "limpeza".into(),
                action: PickerAction::TrimTrailingSpaces,
            },
            PickerItem {
                label: "Ir para linha...".into(),
                detail: "Ctrl+G".into(),
                action: PickerAction::GotoLine,
            },
            PickerItem {
                label: "Salvar como...".into(),
                detail: "novo caminho".into(),
                action: PickerAction::SaveAs,
            },
            PickerItem {
                label: "Nova aba em branco".into(),
                detail: "Ctrl+N".into(),
                action: PickerAction::NewBuffer,
            },
            PickerItem {
                label: "Dividir Painel Lado a Lado (Esquerda │ Direita)".into(),
                detail: "Ctrl+\\ · Ctrl+K \\".into(),
                action: PickerAction::SplitHorizontal,
            },
            PickerItem {
                label: "Dividir Painel Acima / Abaixo (Topo ─ Base)".into(),
                detail: "Ctrl+K -".into(),
                action: PickerAction::SplitVertical,
            },
            PickerItem {
                label: "Inverter Orientacao da Divisao (│ <-> ─)".into(),
                detail: "Ctrl+K o · Shift+Alt+0".into(),
                action: PickerAction::ToggleSplitOrientation,
            },
            PickerItem {
                label: "Mover Aba para Divisao a Direita (→)".into(),
                detail: "Ctrl+Alt+→ · Ctrl+K Shift+→".into(),
                action: PickerAction::MoveTabRight,
            },
            PickerItem {
                label: "Mover Aba para Divisao a Esquerda (←)".into(),
                detail: "Ctrl+Alt+← · Ctrl+K Shift+←".into(),
                action: PickerAction::MoveTabLeft,
            },
            PickerItem {
                label: "Mover Aba para Divisao Abaixo (↓)".into(),
                detail: "Ctrl+K Shift+↓".into(),
                action: PickerAction::MoveTabDown,
            },
            PickerItem {
                label: "Mover Aba para Divisao Acima (↑)".into(),
                detail: "Ctrl+K Shift+↑".into(),
                action: PickerAction::MoveTabUp,
            },
            PickerItem {
                label: "Trocar Paineis de Lugar (Swap)".into(),
                detail: "Ctrl+K s".into(),
                action: PickerAction::SwapPanes,
            },
            PickerItem {
                label: "Redimensionar Painel (Modo Interativo 2D)".into(),
                detail: "Ctrl+K r · Alt+=/Alt+-".into(),
                action: PickerAction::ResizePaneMode,
            },
            PickerItem {
                label: "Igualar Tamanho dos Paineis (50/50)".into(),
                detail: "Ctrl+K =".into(),
                action: PickerAction::EqualizePanes,
            },
            PickerItem {
                label: "Fechar Painel Atual".into(),
                detail: "Ctrl+K w".into(),
                action: PickerAction::ClosePane,
            },
            PickerItem {
                label: "Recarregar arquivo do disco".into(),
                detail: "F5".into(),
                action: PickerAction::ReloadFromDisk,
            },
            PickerItem {
                label: "Paleta: Cyber (Obsidian Ciano/Violeta)".into(),
                detail: "Alt+1".into(),
                action: PickerAction::SetPalette(0),
            },
            PickerItem {
                label: "Paleta: Emerald (Obsidian Esmeralda)".into(),
                detail: "Alt+2".into(),
                action: PickerAction::SetPalette(1),
            },
            PickerItem {
                label: "Paleta: Sunset (Obsidian Magma/Ouro)".into(),
                detail: "Alt+3".into(),
                action: PickerAction::SetPalette(2),
            },
            PickerItem {
                label: "Paleta: Nord (Obsidian Aurora)".into(),
                detail: "Alt+4".into(),
                action: PickerAction::SetPalette(3),
            },
            PickerItem {
                label: "Paleta: Stealth (Obsidian Ametista)".into(),
                detail: "Alt+5".into(),
                action: PickerAction::SetPalette(4),
            },
            PickerItem {
                label: "Paleta: Dracula Redefined (VS Code)".into(),
                detail: "Alt+6".into(),
                action: PickerAction::SetPalette(5),
            },
        ];
        let mut p = Self {
            title: "Command Palette".into(),
            query: String::new(),
            items,
            filtered: Vec::new(),
            selected: 0,
        };
        p.refilter();
        p
    }

    pub fn refilter(&mut self) {
        self.filtered.clear();
        if self.query.is_empty() {
            for i in 0..self.items.len() {
                self.filtered.push(ScoredItem {
                    index: i,
                    score: 0,
                    matched_chars: Vec::new(),
                });
            }
        } else {
            for (i, item) in self.items.iter().enumerate() {
                if let Some((score, matched_chars)) = fuzzy_score(&self.query, &item.label) {
                    self.filtered.push(ScoredItem {
                        index: i,
                        score,
                        matched_chars,
                    });
                }
            }
            self.filtered.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.index.cmp(&b.index)));
        }
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
    }

    pub fn move_sel(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            return;
        }
        let n = self.filtered.len() as isize;
        self.selected = ((self.selected as isize + delta).rem_euclid(n)) as usize;
    }

    pub fn chosen(&self) -> Option<PickerAction> {
        self.filtered
            .get(self.selected)
            .and_then(|s| self.items.get(s.index))
            .map(|it| it.action.clone())
    }
}

pub fn fuzzy_score(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    let q_chars: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).collect();
    if q_chars.is_empty() {
        return Some((0, Vec::new()));
    }
    let t_chars: Vec<char> = text.chars().collect();
    let t_lower: Vec<char> = t_chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();

    let mut matched = Vec::with_capacity(q_chars.len());
    let mut qi = 0usize;
    let mut score = 0i32;
    let mut prev_match: Option<usize> = None;
    let last_slash = t_chars.iter().rposition(|&c| c == '/');

    for (ti, &tc) in t_lower.iter().enumerate() {
        if tc == q_chars[qi] {
            matched.push(ti);
            score += 10;
            if let Some(pm) = prev_match {
                if ti == pm + 1 {
                    score += 15; // Consecutive run bonus
                }
            }
            if ti == 0 || matches!(t_chars[ti - 1], '/' | '_' | '-' | '.' | ' ') {
                score += 12; // Word boundary bonus
            }
            if last_slash.map_or(true, |ls| ti > ls) {
                score += 6; // Basename priority
            }
            prev_match = Some(ti);
            qi += 1;
            if qi == q_chars.len() {
                score -= (t_chars.len() as i32) / 4;
                return Some((score, matched));
            }
        }
    }
    None
}

fn find_project_root(base: &Path) -> Option<PathBuf> {
    let out = Command::new("git")
        .current_dir(base)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(PathBuf::from(s));
        }
    }
    None
}

fn walk_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>, depth: usize, limit: usize) {
    if depth > 8 || out.len() >= limit {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        if out.len() >= limit {
            return;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if matches!(
            name_str.as_ref(),
            ".git" | "node_modules" | "target" | ".venv" | "venv" | "__pycache__" | ".next" | "dist" | "build"
        ) {
            continue;
        }
        let ft = match entry.file_type() {
            Ok(f) => f,
            Err(_) => continue,
        };
        if ft.is_dir() {
            walk_files(root, &path, out, depth + 1, limit);
        } else if ft.is_file() {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_path_buf());
            }
        }
    }
}
