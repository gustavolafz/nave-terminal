use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct PickerItem {
    pub label: String,
    pub detail: String,
    pub action: PickerAction,
}

#[derive(Clone, Debug)]
pub enum PickerAction {
    OpenFile(PathBuf),
    BrowseDir(PathBuf),
    JumpToLine(usize),
    InsertSnippet(String),
    ToggleVimVisual,
    CycleWtBgMode,
    SetWtBgFrosted,
    SetWtBgSolid,
    SetWtBgGlass,
    ToggleOpaqueBg,
    ToggleRelativeNumbers,
    ToggleBlockCursor,
    ToggleCursorTrail,
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
    ToggleAutocomplete,
    ToggleAutocompletePopup,
    CycleAutocompleteMinLen,
    ToggleCpRadar,
    ToggleExamMode,
    JudgeRunRelease,
    JudgeRunDebug,
    NextDiagnostic,
    CpLayoutSetup,
    OpenSnippetsPicker,
    StressTestRun,
    StressTestScaffold,
    StressTestCancel,
    GotoSymbol,
    JumpMatchingBracket,
    CopyLeetCodeSolution,
    TimerStopwatch,
    TimerCountdown(u64),
    TimerTogglePause,
    TimerStop,
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
    ToggleTerminal,
    RestartTerminal,
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
    pub title_prefix: Option<String>,
    pub query: String,
    pub items: Vec<PickerItem>,
    pub filtered: Vec<ScoredItem>,
    pub selected: usize,
    pub browse_dir: Option<PathBuf>,
    pub create_base: Option<PathBuf>,
}

impl Picker {
    pub fn quick_open(base_dir: &Path) -> Self {
        Self::quick_open_titled(base_dir, None)
    }

    pub fn quick_open_titled(base_dir: &Path, title_prefix: Option<&str>) -> Self {
        let abs_base = fs::canonicalize(base_dir).unwrap_or_else(|_| base_dir.to_path_buf());
        let root = find_project_root(&abs_base).unwrap_or(abs_base);
        let mut p = Self {
            title: String::new(),
            title_prefix: title_prefix.map(|s| s.to_string()),
            query: String::new(),
            items: Vec::new(),
            filtered: Vec::new(),
            selected: 0,
            browse_dir: None,
            create_base: None,
        };
        p.browse_to(&root);
        p
    }

    pub fn browse_to(&mut self, dir: &Path) {
        let abs = fs::canonicalize(dir).unwrap_or_else(|_| normalize_path(dir));
        let mut items = Vec::new();

        // 1. Parent directory navigation entry (`../`) always at top when available
        if let Some(parent) = abs.parent() {
            let parent_path = parent.to_path_buf();
            let parent_name = parent
                .file_name()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("/");
            items.push(PickerItem {
                label: "../".to_string(),
                detail: format!("↑ pasta mae ({})", parent_name),
                action: PickerAction::BrowseDir(parent_path),
            });
        }

        // 2. Direct subdirectories (sorted) so user can enter any sibling/child folder
        let mut subdirs = Vec::new();
        let mut direct_files = Vec::new();
        if let Ok(entries) = fs::read_dir(&abs) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if matches!(name.as_str(), ".git" | "node_modules" | "target" | "__pycache__") {
                    continue;
                }
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if ft.is_dir() {
                    subdirs.push((name, path));
                } else if ft.is_file() {
                    direct_files.push((name, path));
                }
            }
        }
        subdirs.sort_by(|a, b| a.0.cmp(&b.0));
        direct_files.sort_by(|a, b| a.0.cmp(&b.0));

        for (name, path) in subdirs {
            items.push(PickerItem {
                label: format!("{}/", name),
                detail: "pasta ▸".to_string(),
                action: PickerAction::BrowseDir(path),
            });
        }

        // 3. Direct files first, then nested files (time-bounded so `/` or `~` never lags)
        for (name, path) in direct_files {
            items.push(PickerItem {
                label: name,
                detail: String::new(),
                action: PickerAction::OpenFile(path),
            });
        }

        let mut nested_files = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(60);
        walk_nested_files(&abs, &abs, &mut nested_files, 0, 4_000, deadline);
        nested_files.sort();
        for rel in nested_files {
            let full = abs.join(&rel);
            items.push(PickerItem {
                label: rel.to_string_lossy().into_owned(),
                detail: String::new(),
                action: PickerAction::OpenFile(full),
            });
        }

        let dir_lbl = compact_dir_label(&abs);
        self.title = match self.title_prefix.as_deref() {
            Some(pref) => format!("{} · {}", pref, dir_lbl),
            None => format!("Abrir Arquivo · {}", dir_lbl),
        };
        self.query.clear();
        self.items = items;
        self.selected = 0;
        self.browse_dir = Some(abs.clone());
        self.create_base = Some(abs);
        self.refilter();
    }

    pub fn go_parent(&mut self) -> bool {
        let Some(parent) = self
            .browse_dir
            .as_ref()
            .and_then(|d| d.parent().map(Path::to_path_buf))
        else {
            return false;
        };
        self.browse_to(&parent);
        true
    }

    pub fn symbols_picker(symbols: &[crate::cp_assist::SymbolEntry]) -> Self {
        let items = symbols
            .iter()
            .map(|s| PickerItem {
                label: format!("[{}] {}", s.kind, s.name),
                detail: format!("Ln {}", s.line),
                action: PickerAction::JumpToLine(s.line),
            })
            .collect();
        let mut p = Self {
            title: "Ir para Simbolo (Ctrl+Shift+O)".into(),
            title_prefix: None,
            query: String::new(),
            items,
            filtered: Vec::new(),
            selected: 0,
            browse_dir: None,
            create_base: None,
        };
        p.refilter();
        p
    }

    pub fn snippets_picker(snippets: &[crate::cp_assist::Snippet]) -> Self {
        let items = snippets
            .iter()
            .map(|s| PickerItem {
                label: s.trigger.clone(),
                detail: s.description.clone(),
                action: PickerAction::InsertSnippet(s.body.clone()),
            })
            .collect();
        let mut p = Self {
            title: "Templates / Snippets CP (~/.config/nave/snippets)".into(),
            title_prefix: None,
            query: String::new(),
            items,
            filtered: Vec::new(),
            selected: 0,
            browse_dir: None,
            create_base: None,
        };
        p.refilter();
        p
    }

    pub fn command_palette(
        cfg: &crate::config::EditorConfig,
        eol: &str,
        wt_mode: Option<crate::config::WtBgMode>,
    ) -> Self {
        use crate::config::WtBgMode;
        let onoff = |b: bool| if b { "ON" } else { "OFF" };
        let wt_lbl = match wt_mode {
            Some(WtBgMode::Glass(op)) => format!("Modo de Fundo (Terminal) [Acrylic {}%]", op),
            Some(WtBgMode::Frosted) => "Modo de Fundo (Terminal) [Vidro Escuro]".into(),
            Some(WtBgMode::Solid) => "Modo de Fundo (Terminal) [Solido 100%]".into(),
            None => "Alternar Modo de Fundo (Terminal)".into(),
        };
        let items = vec![
            PickerItem {
                label: wt_lbl,
                detail: "Alt+O · cicla 3 modos".into(),
                action: PickerAction::CycleWtBgMode,
            },
            PickerItem {
                label: "Fundo: Vidro Escuro (Frosted 100%)".into(),
                detail: "imune a tela branca".into(),
                action: PickerAction::SetWtBgFrosted,
            },
            PickerItem {
                label: "Fundo: Obsidiana Solido (100%)".into(),
                detail: "sem translucidez".into(),
                action: PickerAction::SetWtBgSolid,
            },
            PickerItem {
                label: "Fundo: Translucido Real (Acrylic)".into(),
                detail: "transparencia real".into(),
                action: PickerAction::SetWtBgGlass,
            },
            PickerItem {
                label: format!("Fundo Solido no Editor [{}]", onoff(cfg.opaque_bg)),
                detail: "oculta wallpaper".into(),
                action: PickerAction::ToggleOpaqueBg,
            },
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
                label: format!("Animacao de Trajeto do Cursor [{}]", onoff(cfg.cursor_trail)),
                detail: "Ctrl+setas · Ctrl+1..9 · clique".into(),
                action: PickerAction::ToggleCursorTrail,
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
                label: "Alternar Terminal Integrado (VS Code)".into(),
                detail: "Ctrl+' · Ctrl+K t".into(),
                action: PickerAction::ToggleTerminal,
            },
            PickerItem {
                label: "Novo / Reiniciar Terminal Integrado".into(),
                detail: "abre shell limpo na pasta atual".into(),
                action: PickerAction::RestartTerminal,
            },
            PickerItem {
                label: "CP: Compilar e Julgar Casos (O2 · -DLOCAL)".into(),
                detail: "F5 (in.txt / in1.txt / ---)".into(),
                action: PickerAction::JudgeRunRelease,
            },
            PickerItem {
                label: "CP: Compilar e Julgar DEBUG (ASan + UBSan)".into(),
                detail: "F6 (pega out-of-bounds/UB)".into(),
                action: PickerAction::JudgeRunDebug,
            },
            PickerItem {
                label: "CP: Proximo Erro / Warning de Compilacao".into(),
                detail: "F8".into(),
                action: PickerAction::NextDiagnostic,
            },
            PickerItem {
                label: "CP: Montar Layout Maratona (sol | in.txt / expected.txt)".into(),
                detail: "ne --cp · 3 paineis".into(),
                action: PickerAction::CpLayoutSetup,
            },
            PickerItem {
                label: "CP: Inserir Template / Snippet (dsu, segtree, bit, dijk...)".into(),
                detail: "ou digite gatilho + Tab".into(),
                action: PickerAction::OpenSnippetsPicker,
            },
            PickerItem {
                label: "CP: Rodar Stress Test (gen.cpp + brute.cpp + sol)".into(),
                detail: "salva contra-exemplo em in.txt".into(),
                action: PickerAction::StressTestRun,
            },
            PickerItem {
                label: "CP: Gerar Esqueleto Stress (gen.cpp + brute.cpp)".into(),
                detail: "cria arquivos na pasta atual".into(),
                action: PickerAction::StressTestScaffold,
            },
            PickerItem {
                label: "CP: Parar Judge / Stress Test em andamento".into(),
                detail: "cancela execucao".into(),
                action: PickerAction::StressTestCancel,
            },
            PickerItem {
                label: "CP: Copiar Solution Limpa p/ LeetCode (Clipboard)".into(),
                detail: "remove #ifdef LOCAL e dbg()".into(),
                action: PickerAction::CopyLeetCodeSolution,
            },
            PickerItem {
                label: "Ir para Simbolo / Funcao / Struct".into(),
                detail: "Ctrl+Shift+O".into(),
                action: PickerAction::GotoSymbol,
            },
            PickerItem {
                label: "Saltar para Par de Delimitador ({} () [])".into(),
                detail: "Ctrl+] · Alt+M".into(),
                action: PickerAction::JumpMatchingBracket,
            },
            PickerItem {
                label: format!("Autocomplete: Popup de Dicas [{}]", onoff(cfg.autocomplete_popup)),
                detail: "OFF = so texto fantasma (Ctrl+Space abre)".into(),
                action: PickerAction::ToggleAutocompletePopup,
            },
            PickerItem {
                label: format!("Autocomplete / Dicas [{}]", onoff(cfg.autocomplete)),
                detail: format!("liga/desliga sugestoes (min {} chars)", cfg.autocomplete_min_len),
                action: PickerAction::ToggleAutocomplete,
            },
            PickerItem {
                label: format!("Autocomplete: Minimo de Caracteres [{}]", cfg.autocomplete_min_len),
                detail: "3 / 4 / 5 / 6 chars".into(),
                action: PickerAction::CycleAutocompleteMinLen,
            },
            PickerItem {
                label: format!("CP: Radar Anti-Armadilhas (endl, 1<<31, hash) [{}]", onoff(cfg.cp_radar)),
                detail: "marca '!' no gutter em 0ms".into(),
                action: PickerAction::ToggleCpRadar,
            },
            PickerItem {
                label: format!("CP: Modo Prova / ICPC (bloqueia snippets prontos) [{}]", onoff(cfg.exam_mode)),
                detail: "treino fiel a maratona".into(),
                action: PickerAction::ToggleExamMode,
            },
            PickerItem {
                label: "Timer CP: Iniciar Cronometro (00:00 ↑)".into(),
                detail: "barra de status".into(),
                action: PickerAction::TimerStopwatch,
            },
            PickerItem {
                label: "Timer CP: Contagem Regressiva 25 min (LeetCode)".into(),
                detail: "simulado rapido".into(),
                action: PickerAction::TimerCountdown(25),
            },
            PickerItem {
                label: "Timer CP: Contagem Regressiva 45 min (Entrevista)".into(),
                detail: "simulado medio".into(),
                action: PickerAction::TimerCountdown(45),
            },
            PickerItem {
                label: "Timer CP: Contagem Regressiva 2h (Codeforces Round)".into(),
                detail: "120 minutos".into(),
                action: PickerAction::TimerCountdown(120),
            },
            PickerItem {
                label: "Timer CP: Pausar / Retomar".into(),
                detail: "alterna pausa".into(),
                action: PickerAction::TimerTogglePause,
            },
            PickerItem {
                label: "Timer CP: Desligar / Zerar".into(),
                detail: "oculta da barra".into(),
                action: PickerAction::TimerStop,
            },
            PickerItem {
                label: "Recarregar arquivo do disco".into(),
                detail: "F9 · Shift+F5".into(),
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
            title_prefix: None,
            query: String::new(),
            items,
            filtered: Vec::new(),
            selected: 0,
            browse_dir: None,
            create_base: None,
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
        if let Some(act) = self
            .filtered
            .get(self.selected)
            .and_then(|s| self.items.get(s.index))
            .map(|it| it.action.clone())
        {
            return Some(act);
        }
        // No existing file matched: allow creating/opening a new file directly from the typed path!
        if let Some(ref base) = self.create_base {
            let q = self.query.trim();
            if !q.is_empty() {
                let raw = if Path::new(q).is_absolute() {
                    PathBuf::from(q)
                } else {
                    base.join(q)
                };
                let target = normalize_path(&raw);
                if target.is_dir() {
                    return Some(PickerAction::BrowseDir(target));
                }
                return Some(PickerAction::OpenFile(target));
            }
        }
        None
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(comp.as_os_str());
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

fn compact_dir_label(dir: &Path) -> String {
    let raw = if let Ok(home) = env::var("HOME") {
        let hp = Path::new(&home);
        if dir == hp {
            "~".to_string()
        } else if let Ok(rel) = dir.strip_prefix(hp) {
            format!("~/{}", rel.to_string_lossy())
        } else {
            dir.to_string_lossy().into_owned()
        }
    } else {
        dir.to_string_lossy().into_owned()
    };
    let chars: Vec<char> = raw.chars().collect();
    if chars.len() <= 32 {
        raw
    } else {
        let tail: String = chars[chars.len() - 30..].iter().collect();
        format!("…{}", tail)
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

fn walk_nested_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<PathBuf>,
    depth: usize,
    limit: usize,
    deadline: Instant,
) {
    if depth > 6 || out.len() >= limit || Instant::now() >= deadline {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        if out.len() >= limit || Instant::now() >= deadline {
            return;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if matches!(
            name_str.as_ref(),
            ".git"
                | "node_modules"
                | "target"
                | ".venv"
                | "venv"
                | "__pycache__"
                | ".next"
                | "dist"
                | "build"
                | ".cache"
                | ".cargo"
                | ".rustup"
                | ".npm"
                | ".nvm"
                | ".local"
                | ".vscode-server"
                | "proc"
                | "sys"
                | "dev"
                | "run"
                | "snap"
        ) {
            continue;
        }
        let ft = match entry.file_type() {
            Ok(f) => f,
            Err(_) => continue,
        };
        if ft.is_dir() {
            if depth >= 1 && name_str.starts_with('.') {
                continue;
            }
            walk_nested_files(root, &path, out, depth + 1, limit, deadline);
        } else if ft.is_file() && depth >= 1 {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_path_buf());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_open_navigates_parent_and_child_dirs() {
        let base = env::temp_dir().join(format!("nave_picker_test_{}", std::process::id()));
        let child_a = base.join("pasta_a");
        let child_b = base.join("pasta_b");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&child_a).unwrap();
        fs::create_dir_all(&child_b).unwrap();
        fs::write(child_a.join("um.txt"), "1").unwrap();
        fs::write(child_b.join("dois.txt"), "2").unwrap();
        fs::write(base.join("raiz.txt"), "0").unwrap();

        let mut pk = Picker::quick_open_titled(&child_a, Some("Painel 2"));
        assert_eq!(pk.items[0].label, "../");
        assert!(matches!(pk.items[0].action, PickerAction::BrowseDir(_)));

        assert!(pk.go_parent());
        assert_eq!(pk.browse_dir.as_deref(), Some(fs::canonicalize(&base).unwrap().as_path()));
        assert!(pk.title.starts_with("Painel 2 · "));
        assert!(pk.items.iter().any(|it| it.label == "pasta_b/"));
        assert!(pk.items.iter().any(|it| it.label == "raiz.txt"));

        // Enter pasta_b
        pk.browse_to(&child_b);
        assert!(pk.items.iter().any(|it| it.label == "dois.txt"));

        // Relative creation after navigating to parent resolves cleanly
        assert!(pk.go_parent());
        pk.query = "novo_na_mae.rs".into();
        pk.refilter();
        match pk.chosen() {
            Some(PickerAction::OpenFile(p)) => assert_eq!(p.file_name().unwrap(), "novo_na_mae.rs"),
            other => panic!("Expected OpenFile for new file in parent, got {:?}", other),
        }

        let _ = fs::remove_dir_all(&base);
    }
}
