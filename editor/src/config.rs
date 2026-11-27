use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Clone, Debug)]
pub struct EditorConfig {
    pub palette: String,
    pub opaque_bg: bool,
    pub vim_visual: bool,
    pub relative_numbers: bool,
    pub block_cursor: bool,
    pub auto_hide_mouse: bool,
    pub wheel_moves_cursor: bool,
    pub scrollbar: bool,
    pub center_scroll: bool,
    pub scrolloff: usize,
    pub wrap: bool,
    pub auto_close: bool,
    pub cursorword: bool,
    pub indent_guides: bool,
    pub rainbow_brackets: bool,
    pub bracket_match: bool,
    pub comment_badges: bool,
    pub show_trailing: bool,
    pub color_column: usize, // 0 = off, else 80 / 100 / 120
    pub autocomplete: bool,
    pub autocomplete_min_len: usize, // min identifier length for auto suggestions (default 4)
    pub cp_radar: bool,
    pub exam_mode: bool,
    pub judge_timeout_ms: u64,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            palette: "auto".into(),
            opaque_bg: false,
            vim_visual: false,
            relative_numbers: false,
            block_cursor: false,
            auto_hide_mouse: true,
            wheel_moves_cursor: false,
            scrollbar: true,
            center_scroll: false,
            scrolloff: 3,
            wrap: false,
            auto_close: true,
            cursorword: true,
            indent_guides: true,
            rainbow_brackets: true,
            bracket_match: true,
            comment_badges: true,
            show_trailing: true,
            color_column: 120,
            autocomplete: true,
            autocomplete_min_len: 4,
            cp_radar: true,
            exam_mode: false,
            judge_timeout_ms: 2000,
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
                "palette" => {
                    if !v.is_empty() {
                        cfg.palette = v.to_string();
                    }
                }
                "opaque_bg" => cfg.opaque_bg = parse_bool(v),
                "vim_visual" => cfg.vim_visual = parse_bool(v),
                "relative_numbers" => cfg.relative_numbers = parse_bool(v),
                "block_cursor" => cfg.block_cursor = parse_bool(v),
                "auto_hide_mouse" => cfg.auto_hide_mouse = parse_bool(v),
                "wheel_moves_cursor" | "keyboard_only_nav" => cfg.wheel_moves_cursor = parse_bool(v),
                "mouse" => cfg.wheel_moves_cursor = !parse_bool(v), // legacy config migration
                "scrollbar" => cfg.scrollbar = parse_bool(v),
                "center_scroll" => cfg.center_scroll = parse_bool(v),
                "scrolloff" => {
                    if let Ok(n) = v.parse::<usize>() {
                        cfg.scrolloff = n.min(20);
                    }
                }
                "wrap" => cfg.wrap = parse_bool(v),
                "auto_close" => cfg.auto_close = parse_bool(v),
                "cursorword" => cfg.cursorword = parse_bool(v),
                "indent_guides" => cfg.indent_guides = parse_bool(v),
                "rainbow_brackets" => cfg.rainbow_brackets = parse_bool(v),
                "bracket_match" => cfg.bracket_match = parse_bool(v),
                "comment_badges" => cfg.comment_badges = parse_bool(v),
                "show_trailing" => cfg.show_trailing = parse_bool(v),
                "color_column" => {
                    if let Ok(n) = v.parse::<usize>() {
                        cfg.color_column = n.min(200);
                    }
                }
                "autocomplete" => cfg.autocomplete = parse_bool(v),
                "autocomplete_min_len" => {
                    if let Ok(n) = v.parse::<usize>() {
                        cfg.autocomplete_min_len = n.clamp(3, 8);
                    }
                }
                "cp_radar" => cfg.cp_radar = parse_bool(v),
                "exam_mode" => cfg.exam_mode = parse_bool(v),
                "judge_timeout_ms" => {
                    if let Ok(n) = v.parse::<u64>() {
                        cfg.judge_timeout_ms = n.clamp(500, 30_000);
                    }
                }
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
             palette={}\n\
             opaque_bg={}\n\
             vim_visual={}\n\
             relative_numbers={}\n\
             block_cursor={}\n\
             auto_hide_mouse={}\n\
             wheel_moves_cursor={}\n\
             scrollbar={}\n\
             center_scroll={}\n\
             scrolloff={}\n\
             wrap={}\n\
             auto_close={}\n\
             cursorword={}\n\
             indent_guides={}\n\
             rainbow_brackets={}\n\
             bracket_match={}\n\
             comment_badges={}\n\
             show_trailing={}\n\
             color_column={}\n\
             autocomplete={}\n\
             autocomplete_min_len={}\n\
             cp_radar={}\n\
             exam_mode={}\n\
             judge_timeout_ms={}\n",
            self.palette,
            self.opaque_bg,
            self.vim_visual,
            self.relative_numbers,
            self.block_cursor,
            self.auto_hide_mouse,
            self.wheel_moves_cursor,
            self.scrollbar,
            self.center_scroll,
            self.scrolloff,
            self.wrap,
            self.auto_close,
            self.cursorword,
            self.indent_guides,
            self.rainbow_brackets,
            self.bracket_match,
            self.comment_badges,
            self.show_trailing,
            self.color_column,
            self.autocomplete,
            self.autocomplete_min_len,
            self.cp_radar,
            self.exam_mode,
            self.judge_timeout_ms
        );
        let _ = fs::write(path, body);
    }

    pub fn cycle_autocomplete_min_len(&mut self) {
        self.autocomplete_min_len = match self.autocomplete_min_len {
            3 => 4,
            4 => 5,
            5 => 6,
            _ => 3,
        };
        self.save();
    }

    pub fn toggle_vim_visual(&mut self) {
        self.vim_visual = !self.vim_visual;
        if self.vim_visual {
            self.relative_numbers = true;
            self.block_cursor = false;
            self.wheel_moves_cursor = true;
            self.scrollbar = true;
            self.scrolloff = 8;
        } else {
            self.relative_numbers = false;
            self.block_cursor = false;
            self.wheel_moves_cursor = false;
            self.center_scroll = false;
            self.scrolloff = 3;
        }
        self.save();
    }

    pub fn cycle_color_column(&mut self) {
        self.color_column = match self.color_column {
            0 => 80,
            80 => 100,
            100 => 120,
            _ => 0,
        };
        self.save();
    }
}

pub fn find_wt_settings_json() -> Option<PathBuf> {
    if let Ok(override_p) = std::env::var("NAVE_WT_SETTINGS") {
        let p = PathBuf::from(override_p);
        if p.is_file() {
            return Some(p);
        }
    }
    let users_dir = PathBuf::from("/mnt/c/Users");
    let entries = fs::read_dir(users_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_s = name.to_string_lossy();
        if matches!(name_s.as_ref(), "Public" | "Default" | "Default User" | "All Users") {
            continue;
        }
        let cand = entry
            .path()
            .join("AppData/Local/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json");
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

fn parse_json_bool_field(slice: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{}\"", key);
    let idx = slice.find(&needle)?;
    let rest = slice[idx + needle.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn parse_json_u8_field(slice: &str, key: &str) -> Option<u8> {
    let needle = format!("\"{}\"", key);
    let idx = slice.find(&needle)?;
    let rest = slice[idx + needle.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u8>().ok()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WtBgMode {
    Glass(u8),
    Frosted,
    Solid,
}

fn parse_json_str_field<'a>(slice: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{}\"", key);
    let idx = slice.find(&needle)?;
    let rest = slice[idx + needle.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Fast (<0.1ms) read of Windows Terminal background mode: Glass / Frosted / Solid.
pub fn wt_bg_mode() -> Option<WtBgMode> {
    let path = find_wt_settings_json()?;
    let raw = fs::read_to_string(path).ok()?;
    let region = raw
        .find("\"defaults\"")
        .map(|i| &raw[i..(i + 1800).min(raw.len())])
        .unwrap_or(&raw);
    let ac = parse_json_bool_field(region, "useAcrylic").unwrap_or(false);
    let op = parse_json_u8_field(region, "opacity").unwrap_or(100);
    let sh = parse_json_str_field(region, "experimental.pixelShaderPath").unwrap_or("");
    if ac || op < 100 {
        Some(WtBgMode::Glass(op))
    } else if sh.to_ascii_lowercase().contains("nave-frosted.hlsl") {
        Some(WtBgMode::Frosted)
    } else {
        Some(WtBgMode::Solid)
    }
}

/// Cycles (`target = None`: Glass -> Frosted -> Solid -> Glass) or sets (`Some("frosted"|"solid"|"glass")`)
/// Windows Terminal background mode live.
pub fn set_wt_bg_mode(target: Option<&str>) -> Result<WtBgMode, String> {
    let wt_path = find_wt_settings_json()
        .ok_or_else(|| "settings.json do Windows Terminal nao encontrado".to_string())?;
    let cfg_dir = EditorConfig::config_path()
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let _ = fs::create_dir_all(&cfg_dir);
    let mode_arg = target.unwrap_or("cycle");

    let py = r#"
import json, pathlib, sys
wt_path = pathlib.Path(sys.argv[1])
cfg_dir = pathlib.Path(sys.argv[2])
req = sys.argv[3].lower()
op_file = cfg_dir / "glass_opacity"
prev_sh_file = cfg_dir / "prev_shader"

# Derive Windows user folder from settings.json path if under /mnt/c/Users/<user>/...
parts = wt_path.parts
win_user = "gusta"
for i, part in enumerate(parts):
    if part.lower() == "users" and i + 1 < len(parts):
        win_user = parts[i + 1]
        break
shader_dir = f"C:\\Users\\{win_user}\\.config\\nave\\shaders"
frosted_sh = f"{shader_dir}\\nave-frosted.hlsl"
default_sh = f"{shader_dir}\\nave-glow.hlsl"

data = json.loads(wt_path.read_text(encoding="utf-8"))
profiles = data.setdefault("profiles", {})
defaults = profiles.setdefault("defaults", {})
plist = profiles.get("list", [])
targets = ("Ubuntu", "Windows PowerShell", "PowerShell")
sources = ("Microsoft.WSL", "Windows.Terminal.PowershellCore")

ref = next((p for p in plist if p.get("name") == "Ubuntu"), defaults)
ac = bool(ref.get("useAcrylic", defaults.get("useAcrylic", False)))
op = int(ref.get("opacity", defaults.get("opacity", 100)))
cur_sh = str(ref.get("experimental.pixelShaderPath", defaults.get("experimental.pixelShaderPath", "")) or "")
is_glass = ac or op < 100
is_frosted = (not ac and op >= 100 and "nave-frosted.hlsl" in cur_sh.lower())
cur_mode = "glass" if is_glass else ("frosted" if is_frosted else "solid")

if req == "cycle":
    tgt = {"glass": "frosted", "frosted": "solid", "solid": "glass"}[cur_mode]
else:
    tgt = req

def restore_shader():
    if "nave-frosted.hlsl" not in cur_sh.lower():
        return cur_sh if cur_sh else None
    try:
        saved = prev_sh_file.read_text(encoding="utf-8").strip()
        if saved == "off":
            return None
        if saved:
            return saved
    except Exception:
        pass
    return default_sh

if tgt == "frosted":
    if op < 100:
        op_file.write_text(f"{op}\n", encoding="utf-8")
    if "nave-frosted.hlsl" not in cur_sh.lower():
        prev_sh_file.write_text((cur_sh if cur_sh else "off") + "\n", encoding="utf-8")
    new_ac, new_op, new_sh = False, 100, frosted_sh
elif tgt == "solid":
    if op < 100:
        op_file.write_text(f"{op}\n", encoding="utf-8")
    new_ac, new_op, new_sh = False, 100, restore_shader()
else:
    try:
        saved = int(op_file.read_text(encoding="utf-8").strip())
        new_op = saved if 40 <= saved < 100 else 92
    except Exception:
        new_op = 92
    op_file.write_text(f"{new_op}\n", encoding="utf-8")
    new_ac, new_sh = True, restore_shader()

matched = [defaults]
for p in plist:
    if (
        p.get("name") in targets
        or p.get("source") in sources
        or "useAcrylic" in p
        or "opacity" in p
        or "experimental.pixelShaderPath" in p
    ):
        matched.append(p)
for p in matched:
    p["useAcrylic"] = new_ac
    p["opacity"] = new_op
    if new_sh is None:
        p.pop("experimental.pixelShaderPath", None)
    else:
        p["experimental.pixelShaderPath"] = new_sh

wt_path.write_text(json.dumps(data, indent=4, ensure_ascii=False) + "\n", encoding="utf-8")
print(f"{tgt} {new_op}")
"#;

    let out = Command::new("python3")
        .arg("-c")
        .arg(py)
        .arg(&wt_path)
        .arg(&cfg_dir)
        .arg(mode_arg)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Falha ao rodar python3: {}", e))?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "Erro ao atualizar settings.json".into()
        } else {
            err
        });
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut parts = stdout.split_whitespace();
    let kind = parts.next().unwrap_or("solid");
    let op = parts.next().and_then(|s| s.parse::<u8>().ok()).unwrap_or(92);
    Ok(match kind {
        "glass" => WtBgMode::Glass(op),
        "frosted" => WtBgMode::Frosted,
        _ => WtBgMode::Solid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wt_bg_mode_cycles_glass_frosted_solid_and_restores_shader_and_opacity() {
        let dir = std::env::temp_dir().join(format!("nave_wt_glass_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let wt_p = dir.join("settings.json");
        fs::write(
            &wt_p,
            r#"{
    "profiles": {
        "defaults": {
            "opacity": 88,
            "useAcrylic": true,
            "experimental.pixelShaderPath": "C:\\Users\\gusta\\.config\\nave\\shaders\\nave-glow.hlsl"
        },
        "list": [
            {
                "name": "Ubuntu",
                "opacity": 88,
                "useAcrylic": true,
                "experimental.pixelShaderPath": "C:\\Users\\gusta\\.config\\nave\\shaders\\nave-glow.hlsl"
            }
        ]
    }
}
"#,
        )
        .unwrap();

        std::env::set_var("NAVE_WT_SETTINGS", &wt_p);
        std::env::set_var("XDG_CONFIG_HOME", &dir);

        assert_eq!(wt_bg_mode(), Some(WtBgMode::Glass(88)));
        assert_eq!(set_wt_bg_mode(None).unwrap(), WtBgMode::Frosted);
        assert_eq!(wt_bg_mode(), Some(WtBgMode::Frosted));

        assert_eq!(set_wt_bg_mode(None).unwrap(), WtBgMode::Solid);
        assert_eq!(wt_bg_mode(), Some(WtBgMode::Solid));
        let solid_raw = fs::read_to_string(&wt_p).unwrap();
        assert!(solid_raw.contains("nave-glow.hlsl"));

        assert_eq!(set_wt_bg_mode(None).unwrap(), WtBgMode::Glass(88));
        assert_eq!(wt_bg_mode(), Some(WtBgMode::Glass(88)));

        std::env::remove_var("NAVE_WT_SETTINGS");
        std::env::remove_var("XDG_CONFIG_HOME");
        let _ = fs::remove_dir_all(&dir);
    }
}
