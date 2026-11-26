#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NAVE_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/nave"
INSTALL_CLI=false

for arg in "$@"; do
  case "$arg" in
    --with-cli) INSTALL_CLI=true ;;
    -h|--help)
      cat <<'EOF'
Nave Terminal v4 Installer (100% Standalone — Zero Oh My Zsh, Zero External Zsh Plugins)
Usage: ./install.sh [--with-cli]

  --with-cli   Also downloads optional modern CLI enhancers (fzf, bat, eza, delta,
               lazygit, btop, fd, rg) into ~/.local/bin (Linux x86_64, no sudo)
EOF
      exit 0
      ;;
  esac
done

echo "╭─ Nave Terminal v4 Installer (Standalone) ───────────────"

if ! command -v zsh >/dev/null 2>&1; then
  echo "│ [!] Zsh nao encontrado. Instale o zsh primeiro (ex: sudo apt install zsh)."
  echo "╰─────────────────────────────────────────────────────────"
  exit 1
fi

mkdir -p "$NAVE_DIR" "$HOME/.local/bin"

# 1. Install all native Nave Zsh modules (100% standalone, zero third-party Zsh plugins)
cp "$REPO_DIR/config/init.zsh"        "$NAVE_DIR/init.zsh"
cp "$REPO_DIR/theme/nave.zsh-theme"   "$NAVE_DIR/theme.zsh"
cp "$REPO_DIR/config/highlight.zsh"   "$NAVE_DIR/highlight.zsh"
cp "$REPO_DIR/config/suggest.zsh"     "$NAVE_DIR/suggest.zsh"
cp "$REPO_DIR/config/z.zsh"           "$NAVE_DIR/z.zsh"
cp "$REPO_DIR/config/keybindings.zsh" "$NAVE_DIR/keybindings.zsh"
cp "$REPO_DIR/config/toolkit.zsh"     "$NAVE_DIR/toolkit.zsh"

cp "$REPO_DIR/bin/nave-hud"  "$HOME/.local/bin/nave-hud"
cp "$REPO_DIR/bin/nave-warp" "$HOME/.local/bin/nave-warp"
chmod +x "$HOME/.local/bin/nave-hud" "$HOME/.local/bin/nave-warp"

echo "│ [ok] Nave Shell Core instalado em $NAVE_DIR (init, theme, highlight, suggest, z, keybindings, toolkit)"
echo "│ [ok] Cockpit apps instalados em ~/.local/bin (nave-hud, nave-warp)"
echo "│ [ok] Zero dependencia de Oh My Zsh ou plugins externos"

# 1b. Build & install nave-edit (native Rust terminal editor) if cargo is available
CARGO_BIN="$(command -v cargo 2>/dev/null || echo "$HOME/.cargo/bin/cargo")"
if [[ -x "$CARGO_BIN" && -f "$REPO_DIR/editor/Cargo.toml" ]]; then
  echo "│ [..] Compilando nave-edit (Rust release)..."
  if (cd "$REPO_DIR/editor" && "$CARGO_BIN" build --release >/dev/null 2>&1); then
    cp "$REPO_DIR/editor/target/release/nave-edit" "$HOME/.local/bin/nave-edit.tmp.$$"
    chmod +x "$HOME/.local/bin/nave-edit.tmp.$$"
    mv -f "$HOME/.local/bin/nave-edit.tmp.$$" "$HOME/.local/bin/nave-edit"
    echo "│ [ok] nave-edit (Rust nativo) instalado em ~/.local/bin/nave-edit (alias: ne)"
  else
    echo "│ [warn] Falha ao compilar nave-edit; rode 'cd editor && cargo build --release' para detalhes"
  fi
else
  echo "│ [info] Rust/cargo nao encontrado — pulei nave-edit (instale via https://rustup.rs)"
fi

# 1c. On WSL, compile tiny native Windows host RAM helper (~4.5 KB, ~40ms) via built-in .NET csc.exe
CSC_EXE="/mnt/c/Windows/Microsoft.NET/Framework64/v4.0.30319/csc.exe"
if [[ -x "$CSC_EXE" && -f "$REPO_DIR/windows-terminal/nave-winmem.cs" ]]; then
  WIN_PROFILE="$(cmd.exe /C 'echo %USERPROFILE%' </dev/null 2>/dev/null | tr -d '\r\n' || true)"
  if [[ -n "$WIN_PROFILE" ]]; then
    WIN_NAVE_WSL="$(wslpath -u "$WIN_PROFILE" 2>/dev/null)/.config/nave"
    mkdir -p "$WIN_NAVE_WSL"
    cp "$REPO_DIR/windows-terminal/nave-winmem.cs" "$WIN_NAVE_WSL/nave-winmem.cs"
    if "$CSC_EXE" /nologo /optimize+ \
      "/out:${WIN_PROFILE}\\.config\\nave\\nave-winmem.exe" \
      "${WIN_PROFILE}\\.config\\nave\\nave-winmem.cs" </dev/null >/dev/null 2>&1; then
      echo "│ [ok] Helper nativo Windows RAM compilado em ${WIN_PROFILE}\\.config\\nave\\nave-winmem.exe"
    fi
  fi
fi

# 2. Optional modern CLI enhancers (x86_64 Linux, no sudo required)
if $INSTALL_CLI; then
  echo "│ [..] Instalando ferramentas CLI opcionais em ~/.local/bin..."
  python3 - <<'PY'
import json, os, re, shutil, subprocess, tarfile, tempfile, urllib.request

BIN_DIR = os.path.expanduser("~/.local/bin")
os.makedirs(BIN_DIR, exist_ok=True)
TOOLS = [
    ("fzf", "junegunn/fzf", r"linux_amd64\.tar\.gz$", "fzf"),
    ("bat", "sharkdp/bat", r"x86_64-unknown-linux-musl\.tar\.gz$", "bat"),
    ("eza", "eza-community/eza", r"eza_x86_64-unknown-linux-musl\.tar\.gz$", "eza"),
    ("delta", "dandavison/delta", r"x86_64-unknown-linux-musl\.tar\.gz$", "delta"),
    ("lazygit", "jesseduffield/lazygit", r"[Ll]inux_x86_64\.tar\.gz$", "lazygit"),
    ("btop", "aristocratos/btop", r"btop-x86_64-unknown-linux-musl\.tbz$", "btop"),
    ("fd", "sharkdp/fd", r"x86_64-unknown-linux-musl\.tar\.gz$", "fd"),
    ("rg", "BurntSushi/ripgrep", r"x86_64-unknown-linux-musl\.tar\.gz$", "rg"),
]

with tempfile.TemporaryDirectory() as tmp:
    for name, repo, pattern, bin_name in TOOLS:
        try:
            req = urllib.request.Request(
                f"https://api.github.com/repos/{repo}/releases/latest",
                headers={"User-Agent": "nave-terminal-installer"}
            )
            with urllib.request.urlopen(req, timeout=20) as r:
                rel = json.loads(r.read())
            asset = next((a for a in rel.get("assets", []) if re.search(pattern, a["name"])), None)
            if not asset:
                print(f"│ [skip] {name}: asset nao encontrado")
                continue
            dl = os.path.join(tmp, asset["name"])
            subprocess.check_call(["curl", "-fsSL", "-o", dl, asset["browser_download_url"]])
            ex = os.path.join(tmp, name)
            os.makedirs(ex, exist_ok=True)
            mode = "r:bz2" if dl.endswith((".tbz", ".bz2")) else "r:gz"
            with tarfile.open(dl, mode) as tf:
                tf.extractall(ex)
            found = None
            for root, _, files in os.walk(ex):
                if bin_name in files and os.path.isfile(os.path.join(root, bin_name)):
                    found = os.path.join(root, bin_name)
                    break
            if found:
                dst = os.path.join(BIN_DIR, bin_name)
                shutil.copy2(found, dst)
                os.chmod(dst, 0o755)
                print(f"│ [ok] {name} instalado")
        except Exception as e:
            print(f"│ [warn] {name}: {e}")
PY
fi

echo "│"
echo "│ Proximo passo no seu ~/.zshrc (apenas 1 linha — veja config/zshrc.example):"
echo "│   export PATH=\"\$HOME/.local/bin:\$HOME/.cargo/bin:\$PATH\""
echo "│   export EDITOR=\"nave-edit\""
echo "│   export NAVE_PROJECTS_DIR=\"\$HOME/projects\""
echo "│   [[ -r \"\$HOME/.config/nave/init.zsh\" ]] && source \"\$HOME/.config/nave/init.zsh\""
echo "│"
echo "│ Depois rode: source ~/.zshrc && nave-help"
echo "╰─────────────────────────────────────────────────────────"
