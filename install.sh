#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ZSH_CUSTOM="${ZSH_CUSTOM:-$HOME/.oh-my-zsh/custom}"
NAVE_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/nave"
INSTALL_CLI=false

for arg in "$@"; do
  case "$arg" in
    --with-cli) INSTALL_CLI=true ;;
    -h|--help)
      cat <<'EOF'
Nave Terminal Installer
Usage: ./install.sh [--with-cli]

  --with-cli   Also downloads modern CLI binaries (fzf, zoxide, bat, eza, delta,
               lazygit, btop, fd, rg) into ~/.local/bin (Linux x86_64)
EOF
      exit 0
      ;;
  esac
done

echo "╭─ Nave Terminal Installer ───────────────────────────────"

if [[ ! -d "$HOME/.oh-my-zsh" ]]; then
  echo "│ [!] Oh My Zsh nao encontrado em ~/.oh-my-zsh."
  echo "│     Instale primeiro: https://ohmyz.sh"
  echo "╰─────────────────────────────────────────────────────────"
  exit 1
fi

mkdir -p "$ZSH_CUSTOM/themes" "$ZSH_CUSTOM/plugins" "$NAVE_DIR"

# 1. Install theme & toolkit
cp "$REPO_DIR/theme/nave.zsh-theme" "$ZSH_CUSTOM/themes/nave.zsh-theme"
cp "$REPO_DIR/config/toolkit.zsh" "$NAVE_DIR/toolkit.zsh"
echo "│ [ok] Tema instalado em $ZSH_CUSTOM/themes/nave.zsh-theme"
echo "│ [ok] Toolkit instalado em $NAVE_DIR/toolkit.zsh"

# 2. Ensure zsh-autosuggestions & zsh-syntax-highlighting exist
if [[ ! -d "$ZSH_CUSTOM/plugins/zsh-autosuggestions" ]]; then
  echo "│ [..] Clonando zsh-autosuggestions..."
  git clone --depth=1 https://github.com/zsh-users/zsh-autosuggestions "$ZSH_CUSTOM/plugins/zsh-autosuggestions" >/dev/null 2>&1
fi
if [[ ! -d "$ZSH_CUSTOM/plugins/zsh-syntax-highlighting" ]]; then
  echo "│ [..] Clonando zsh-syntax-highlighting..."
  git clone --depth=1 https://github.com/zsh-users/zsh-syntax-highlighting "$ZSH_CUSTOM/plugins/zsh-syntax-highlighting" >/dev/null 2>&1
fi
echo "│ [ok] Plugins zsh-autosuggestions e zsh-syntax-highlighting prontos"

# 3. Optional modern CLI install (x86_64 Linux, no sudo required)
if $INSTALL_CLI; then
  echo "│ [..] Instalando ferramentas CLI modernas em ~/.local/bin..."
  python3 - <<'PY'
import json, os, re, shutil, subprocess, tarfile, tempfile, urllib.request

BIN_DIR = os.path.expanduser("~/.local/bin")
os.makedirs(BIN_DIR, exist_ok=True)
TOOLS = [
    ("fzf", "junegunn/fzf", r"linux_amd64\.tar\.gz$", "fzf"),
    ("zoxide", "ajeetdsouza/zoxide", r"x86_64-unknown-linux-musl\.tar\.gz$", "zoxide"),
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
echo "│ Proximos passos no seu ~/.zshrc (veja config/zshrc.example):"
echo "│   1. Defina ZSH_THEME=\"nave\""
echo "│   2. Ative plugins=(git zsh-autosuggestions zsh-syntax-highlighting)"
echo "│   3. Adicione: [[ -r \"\$HOME/.config/nave/toolkit.zsh\" ]] && source \"\$HOME/.config/nave/toolkit.zsh\""
echo "│   4. Rode: source ~/.zshrc && nave-help"
echo "╰─────────────────────────────────────────────────────────"
