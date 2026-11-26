use std::fs;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug)]
pub struct EditorConfig {
    pub vim_visual: bool,
    pub relative_numbers: bool,
    pub block_cursor: bool,
    pub mouse: bool,
    pub scrollbar: bool,
    pub center_scroll: bool,
    pub scrolloff: usize,
    pub wrap: bool,
    pub auto_close: bool,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            vim_visual: false,
            relative_numbers: false,
            block_cursor: false,
            mouse: true,
            scrollbar: true,
            center_scroll: false,
            scrolloff: 3,
            wrap: false,
            auto_close: true,
        }
    }
}

impl EditorConfig {
    pub fn config_path() -> PathBuf {
        std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".config")
            })
            .join("nave/edit.conf")
    }

    pub fn load() -> Self {
        let mut cfg = Self::default();
        let path = Self::config_path();
        let Ok(raw) = fs::read_to_string(path) else {
            return cfg;
        };
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let k = k.trim();
            let v = v.trim();
            let parse_bool = |s: &str| matches!(s, "1" | "true" | "on" | "yes");
            match k {
                "vim_visual" => cfg.vim_visual = parse_bool(v),
                "relative_numbers" => cfg.relative_numbers = parse_bool(v),
                "block_cursor" => cfg.block_cursor = parse_bool(v),
                "mouse" => cfg.mouse = parse_bool(v),
                "scrollbar" => cfg.scrollbar = parse_bool(v),
                "center_scroll" => cfg.center_scroll = parse_bool(v),
                "scrolloff" => {
                    if let Ok(n) = v.parse::<usize>() {
                        cfg.scrolloff = n.min(20);
                    }
                }
                "wrap" => cfg.wrap = parse_bool(v),
                "auto_close" => cfg.auto_close = parse_bool(v),
                _ => {}
            }
        }
        cfg
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let body = format!(
            "# Nave Edit configuration (persisted automatically)\n\
             vim_visual={}\n\
             relative_numbers={}\n\
             block_cursor={}\n\
             mouse={}\n\
             scrollbar={}\n\
             center_scroll={}\n\
             scrolloff={}\n\
             wrap={}\n\
             auto_close={}\n",
            self.vim_visual,
            self.relative_numbers,
            self.block_cursor,
            self.mouse,
            self.scrollbar,
            self.center_scroll,
            self.scrolloff,
            self.wrap,
            self.auto_close
        );
        let _ = fs::write(path, body);
    }

    pub fn toggle_vim_visual(&mut self) {
        self.vim_visual = !self.vim_visual;
        if self.vim_visual {
            self.relative_numbers = true;
            self.block_cursor = true;
            self.mouse = false;
            self.scrollbar = true;
            self.scrolloff = 8;
        } else {
            self.relative_numbers = false;
            self.block_cursor = false;
            self.mouse = true;
            self.center_scroll = false;
            self.scrolloff = 3;
        }
        self.save();
    }
}
