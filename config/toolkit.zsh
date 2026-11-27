# ==============================================================================
# ~/.config/nave/toolkit.zsh
# Modular WSL + Dev + Modern CLI + Art/Cockpit toolkit
# ==============================================================================

: "${NAVE_PROJECTS_DIR:=$HOME/compProjects}"

# ------------------------------------------------------------------------------
# Modern CLI integrations (fzf, zoxide, eza, bat, fd, rg, lazygit, btop)
# ------------------------------------------------------------------------------
if command -v fzf >/dev/null 2>&1; then
  if command -v fd >/dev/null 2>&1; then
    export FZF_DEFAULT_COMMAND='fd --type f --hidden --follow --exclude .git --exclude node_modules'
    export FZF_CTRL_T_COMMAND="$FZF_DEFAULT_COMMAND"
    export FZF_ALT_C_COMMAND='fd --type d --hidden --follow --exclude .git --exclude node_modules'
  fi

  export FZF_CTRL_T_OPTS="--preview 'if [ -d {} ]; then eza --tree --level=2 --color=always {} 2>/dev/null || ls -la {}; else bat --color=always --style=numbers --line-range=:200 {} 2>/dev/null || head -n 200 {}; fi'"
  export FZF_ALT_C_OPTS="--preview 'eza --tree --level=2 --color=always {} 2>/dev/null || ls -la {}'"
  export FZF_CTRL_R_OPTS="--preview 'echo {}' --preview-window=down:3:wrap"

  local fzf_cache="${NAVE_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/nave}/fzf-init.zsh"
  # Avoid `${commands[fzf]}` here: indexing $commands forces Zsh to scan /mnt/c NTFS dirs (~140ms on WSL2!)
  local fzf_bin="$(command -v fzf)"
  if [[ ! -r "$fzf_cache" || "$fzf_bin" -nt "$fzf_cache" ]]; then
    mkdir -p "${fzf_cache:h}"
    fzf --zsh >| "$fzf_cache" 2>/dev/null
    { zcompile "$fzf_cache" 2>/dev/null } &!
  fi
  [[ -r "$fzf_cache" ]] && source "$fzf_cache"
fi

if command -v eza >/dev/null 2>&1; then
  alias ls='eza --color=auto --group-directories-first'
  alias ll='eza -la --git --header --time-style=relative --group-directories-first'
  alias la='eza -a --group-directories-first'
  alias l='eza --group-directories-first'
  function lt {
    local depth="${1:-2}"
    local target="${2:-.}"
    eza --tree --level="$depth" --git-ignore --group-directories-first "$target"
  }
else
  function lt {
    local depth="${1:-2}"
    local target="${2:-.}"
    if command -v tree >/dev/null 2>&1; then
      tree -C -L "$depth" "$target"
    else
      find "$target" -maxdepth "$depth" -not -path '*/.*' | sort | sed -e "s|[^/]*/|│   |g" -e "s|│   \([^│]\)|├── \1|"
    fi
  }
fi

if command -v bat >/dev/null 2>&1; then
  export BAT_THEME="TwoDark"
  alias b='bat --style=numbers,changes'
  alias catp='bat --paging=never --style=plain'
  export MANPAGER="sh -c 'col -bx | bat -l man -p'"
  export MANROFFOPT="-c"
fi

command -v lazygit >/dev/null 2>&1 && alias lg='lazygit'
command -v btop >/dev/null 2>&1 && alias mon='btop'
command -v nave-edit >/dev/null 2>&1 && alias ne='nave-edit'
function nave-cursor-reset {
  local exe="/mnt/c/Users/${USER}/.config/nave/nave-hidemouse.exe"
  [[ -x "$exe" ]] || exe=(/mnt/c/Users/*/.config/nave/nave-hidemouse.exe(N[1]))
  if [[ -n "$exe" && -f "$exe" ]]; then
    "$exe" --reset < /dev/null && print -P "%F{${_NAVE_C[ok]:-48}}Cursor do Windows restaurado.%f"
  fi
}

# ------------------------------------------------------------------------------
# Visual Art Helpers: TrueColor gradient, hr divider, Windows Terminal shaders
# ------------------------------------------------------------------------------
function _nave_grad_line {
  local text="$1"
  local c1="${_NAVE_C[grad1]:-125;207;255}"
  local c2="${_NAVE_C[grad2]:-122;162;247}"
  local c3="${_NAVE_C[grad3]:-187;154;247}"
  local bold="${2:-0}"

  local -a a1=(${(s:;:)c1}) a2=(${(s:;:)c2}) a3=(${(s:;:)c3})
  local r1=${a1[1]} g1=${a1[2]} b1=${a1[3]}
  local r2=${a2[1]} g2=${a2[2]} b2=${a2[3]}
  local r3=${a3[1]} g3=${a3[2]} b3=${a3[3]}

  local len=${#text}
  local denom=$(( len > 1 ? len - 1 : 1 ))
  local out="" i ch t r g b bpre=""
  (( bold )) && bpre="1;"

  for (( i = 1; i <= len; i++ )); do
    ch="${text[$i]}"
    if [[ "$ch" == " " ]]; then
      out+=" "
      continue
    fi
    # Integer interpolation 0..1000
    t=$(( (i - 1) * 1000 / denom ))
    if (( t <= 500 )); then
      r=$(( r1 + (r2 - r1) * t / 500 ))
      g=$(( g1 + (g2 - g1) * t / 500 ))
      b=$(( b1 + (b2 - b1) * t / 500 ))
    else
      local u=$(( t - 500 ))
      r=$(( r2 + (r3 - r2) * u / 500 ))
      g=$(( g2 + (g3 - g2) * u / 500 ))
      b=$(( b2 + (b3 - b2) * u / 500 ))
    fi
    out+=$'\e['"${bpre}38;2;${r};${g};${b}m${ch}"
  done
  printf '%s\e[0m' "$out"
}

# Full-width styled session divider: hr [label]
function hr {
  local label="$*"
  local w="${COLUMNS:-80}"
  local ts
  ts=$(date +%H:%M)
  if [[ -n "$label" ]]; then
    local prefix="─── ${label} "
    local suffix=" ${ts} ───"
    local fill=$(( w - ${#prefix} - ${#suffix} ))
    (( fill < 3 )) && fill=3
    local bar="${(l:${fill}::─:)}"
    _nave_grad_line "${prefix}${bar}${suffix}" 1
    printf '\n'
  else
    local bar="${(l:${w}::─:)}"
    _nave_grad_line "$bar" 0
    printf '\n'
  fi
}

# Switch Windows Terminal HLSL GPU pixel shader live: nave-shader [glow|crt|off|status]
function nave-shader {
  local mode="${1:-status}"
  local wt_json
  wt_json=$(ls /mnt/c/Users/*/AppData/Local/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json 2>/dev/null | head -n 1)
  if [[ -z "$wt_json" || ! -f "$wt_json" ]]; then
    print -P "%F{${_NAVE_C[warn]:-215}}settings.json do Windows Terminal nao encontrado.%f"
    return 1
  fi
  local win_user
  win_user=$(basename "$(dirname "$(dirname "$(dirname "$(dirname "$(dirname "$wt_json")")")")")")
  local shader_dir="C:\\Users\\${win_user}\\.config\\nave\\shaders"

  python3 - "$wt_json" "$mode" "$shader_dir" <<'PY'
import json, pathlib, sys
wt_path, mode, sdir = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
data = json.loads(wt_path.read_text(encoding="utf-8"))
targets = ("Ubuntu", "Windows PowerShell", "PowerShell")
if mode == "status":
    for p in data.get("profiles", {}).get("list", []):
        if p.get("name") in targets:
            print(f"  {p.get('name'):20s} -> {p.get('experimental.pixelShaderPath', 'off')}")
    sys.exit(0)
if mode not in ("glow", "crt", "off"):
    print("Uso: nave-shader <glow|crt|off|status>")
    sys.exit(1)
val = None if mode == "off" else f"{sdir}\\nave-{mode}.hlsl"
for p in data.get("profiles", {}).get("list", []):
    if p.get("name") in targets or p.get("source") in ("Microsoft.WSL", "Windows.Terminal.PowershellCore"):
        if val is None:
            p.pop("experimental.pixelShaderPath", None)
        else:
            p["experimental.pixelShaderPath"] = val
wt_path.write_text(json.dumps(data, indent=4, ensure_ascii=False) + "\n", encoding="utf-8")
print(f"✓ Windows Terminal shader alterado para: {mode}")
PY
}

# Toggle Windows Terminal translucent background (Acrylic/Opacity) & optional wallpaper live:
#   nave-glass             -> alterna Translucido (Acrylic 92%) <-> Solido (100%)
#   nave-glass off|solid   -> tira translucidez (useAcrylic=false, opacity=100)
#   nave-glass on|glass    -> ativa translucidez (restaura opacidade salva, padrao 92%)
#   nave-glass <40..100>   -> define opacidade exata (100 = solido, <100 = acrylic)
#   nave-glass wall        -> alterna imagem de fundo (backgroundImageOpacity 0.68 <-> 0)
#   nave-glass status      -> mostra estado atual
function nave-glass {
  local mode="${1:-toggle}"
  local wt_json="${NAVE_WT_SETTINGS:-}"
  if [[ -z "$wt_json" ]]; then
    wt_json=$(ls /mnt/c/Users/*/AppData/Local/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json 2>/dev/null | head -n 1)
  fi
  if [[ -z "$wt_json" || ! -f "$wt_json" ]]; then
    print -P "%F{${_NAVE_C[warn]:-215}}settings.json do Windows Terminal nao encontrado.%f"
    return 1
  fi
  local cfg_dir="${XDG_CONFIG_HOME:-$HOME/.config}/nave"
  mkdir -p "$cfg_dir" 2>/dev/null

  python3 - "$wt_json" "$mode" "$cfg_dir" <<'PY'
import json, pathlib, sys
wt_path = pathlib.Path(sys.argv[1])
mode = sys.argv[2].lower()
cfg_dir = pathlib.Path(sys.argv[3])
op_file = cfg_dir / "glass_opacity"
wall_file = cfg_dir / "wall_opacity"

data = json.loads(wt_path.read_text(encoding="utf-8"))
profiles = data.setdefault("profiles", {})
defaults = profiles.setdefault("defaults", {})
plist = profiles.get("list", [])
targets = ("Ubuntu", "Windows PowerShell", "PowerShell")
sources = ("Microsoft.WSL", "Windows.Terminal.PowershellCore")

def matched_profiles():
    res = [defaults]
    for p in plist:
        if (
            p.get("name") in targets
            or p.get("source") in sources
            or "useAcrylic" in p
            or "opacity" in p
            or "backgroundImageOpacity" in p
        ):
            res.append(p)
    return res

def cur_state():
    ref = next((p for p in plist if p.get("name") == "Ubuntu"), defaults)
    ac = bool(ref.get("useAcrylic", defaults.get("useAcrylic", False)))
    op = int(ref.get("opacity", defaults.get("opacity", 100)))
    wop = float(ref.get("backgroundImageOpacity", defaults.get("backgroundImageOpacity", 0.0)))
    return ac, op, wop

ac, op, wop = cur_state()
is_glass = ac or op < 100

if mode == "status":
    g_lbl = f"ON (Acrylic · {op}%)" if is_glass else f"OFF (Solido · {op}%)"
    w_lbl = f"ON ({wop:.2f})" if wop > 0.01 else "OFF (0.00)"
    print(f"  Fundo translucido : {g_lbl}")
    print(f"  Wallpaper         : {w_lbl}")
    sys.exit(0)

if mode in ("wall", "wallpaper"):
    if wop > 0.01:
        wall_file.write_text(f"{wop:.2f}\n", encoding="utf-8")
        new_wop = 0.0
        msg = "✓ Wallpaper do Windows Terminal: OFF (fundo liso)"
    else:
        try:
            saved = float(wall_file.read_text(encoding="utf-8").strip())
            new_wop = saved if 0.05 <= saved <= 1.0 else 0.68
        except Exception:
            new_wop = 0.68
        msg = f"✓ Wallpaper do Windows Terminal: ON (opacidade {new_wop:.2f})"
    for p in matched_profiles():
        if "backgroundImage" in p or p is defaults:
            p["backgroundImageOpacity"] = new_wop
    wt_path.write_text(json.dumps(data, indent=4, ensure_ascii=False) + "\n", encoding="utf-8")
    print(msg)
    sys.exit(0)

if mode == "toggle":
    want_glass = not is_glass
    custom_op = None
elif mode in ("off", "solid", "solido", "opaco", "0", "false"):
    want_glass = False
    custom_op = None
elif mode in ("on", "glass", "translucido", "acrylic", "1", "true"):
    want_glass = True
    custom_op = None
elif mode.isdigit():
    val = max(40, min(100, int(mode)))
    want_glass = val < 100
    custom_op = val
else:
    print("Uso: nave-glass [toggle|off|solid|on|glass|wall|status|<40..100>]")
    sys.exit(1)

if not want_glass:
    if op < 100:
        op_file.write_text(f"{op}\n", encoding="utf-8")
    new_ac, new_op = False, 100
    msg = "✓ Modo translucido: OFF (fundo 100% solido)"
else:
    if custom_op is not None:
        new_op = custom_op
    else:
        try:
            saved = int(op_file.read_text(encoding="utf-8").strip())
            new_op = saved if 40 <= saved < 100 else 92
        except Exception:
            new_op = 92
    op_file.write_text(f"{new_op}\n", encoding="utf-8")
    new_ac = True
    msg = f"✓ Modo translucido: ON (Acrylic · opacidade {new_op}%)"

for p in matched_profiles():
    p["useAcrylic"] = new_ac
    p["opacity"] = new_op

wt_path.write_text(json.dumps(data, indent=4, ensure_ascii=False) + "\n", encoding="utf-8")
print(msg)
PY
}

# ------------------------------------------------------------------------------
# WSL <-> Windows Bridge
# ------------------------------------------------------------------------------
function clip {
  if [[ -n "$1" && -f "$1" ]]; then
    clip.exe < "$1"
  else
    clip.exe
  fi
}

function paste {
  powershell.exe -NoProfile -Command "Get-Clipboard" 2>/dev/null | tr -d '\r'
}

function winopen {
  local target="${1:-.}"
  local win_path
  win_path=$(wslpath -w "$target" 2>/dev/null) || { echo "Caminho invalido: $target"; return 1; }
  (explorer.exe "$win_path" >/dev/null 2>&1 &)
}

function winip {
  local wsl_ip host_ip
  wsl_ip=$(hostname -I 2>/dev/null | awk '{print $1}')
  host_ip=$(ip route show default 2>/dev/null | awk '{print $3}')
  print -P "%F{${_NAVE_C[info]:-81}}WSL IP:%f     ${wsl_ip:-N/A}"
  print -P "%F{${_NAVE_C[git]:-141}}Windows IP:%f ${host_ip:-N/A}"
}

function notify {
  if [[ $# -eq 0 ]]; then
    echo "Uso: notify <comando>"
    return 1
  fi
  local t0=$EPOCHSECONDS
  "$@"
  local rc=$?
  local dur=$(( EPOCHSECONDS - t0 ))
  local lbl="Concluido (${dur}s)"
  (( rc != 0 )) && lbl="Falhou [exit ${rc}] (${dur}s)"
  _nave_win_toast "WSL nave — ${lbl}" "$*"
  return $rc
}

# ------------------------------------------------------------------------------
# Port Radar (ports / killport)
# ------------------------------------------------------------------------------
function ports {
  local c_hdr="${_NAVE_C[dir]:-39}"
  local c_dim="${_NAVE_C[frame]:-240}"

  print -P "%F{${c_hdr}}╭─ Portas locais em escuta (TCP/UDP) ─────────────────────────%f"
  printf "  %-6s %-22s %-8s %s\n" "PROTO" "ENDERECO:PORTA" "PID" "PROCESSO"
  print -P "%F{${c_dim}}├─────────────────────────────────────────────────────────────%f"

  local rows
  rows=$(ss -tulpnH 2>/dev/null | awk '
    {
      proto=$1; addr=$5; proc=$7
      pid="-"; name="-"
      if (match(proc, /"([^"]+)"/, m)) name=m[1]
      if (match(proc, /pid=([0-9]+)/, p)) pid=p[1]
      printf "  %-6s %-22s %-8s %s\n", proto, addr, pid, name
    }
  ' | sort -k2 -u)

  if [[ -z "$rows" ]]; then
    print -P "%F{${c_dim}}│%f  (nenhuma porta aberta detectada para este usuario)"
  else
    print -r -- "$rows"
  fi
  print -P "%F{${c_hdr}}╰─────────────────────────────────────────────────────────────%f"
}

function killport {
  local port="$1"
  if [[ -z "$port" ]]; then
    if command -v fzf >/dev/null 2>&1; then
      local picked
      picked=$(ss -tulpnH 2>/dev/null | awk '
        /pid=/ {
          proto=$1; addr=$5; proc=$7
          pid="-"; name="-"
          if (match(proc, /"([^"]+)"/, m)) name=m[1]
          if (match(proc, /pid=([0-9]+)/, p)) pid=p[1]
          split(addr, a, ":")
          pt=a[length(a)]
          printf "%-6s %-6s %-22s %-8s %s\n", proto, pt, addr, pid, name
        }
      ' | sort -k2,2n -k4,4 -u | fzf --header="killport — Selecione a porta para finalizar (Enter)" --prompt="porta ❯ ")
      [[ -z "$picked" ]] && return 0
      port=$(awk '{print $2}' <<< "$picked")
    else
      echo "Uso: killport <porta>"
      return 1
    fi
  fi
  local pids
  pids=$(lsof -ti :"$port" 2>/dev/null)
  if [[ -z "$pids" ]]; then
    pids=$(ss -tulpnH "sport = :$port" 2>/dev/null | grep -oE 'pid=[0-9]+' | cut -d= -f2 | sort -u)
  fi
  if [[ -z "$pids" ]]; then
    print -P "%F{${_NAVE_C[warn]:-215}}Nenhum processo seu encontrado na porta ${port}.%f"
    return 1
  fi
  for pid in ${(f)pids}; do
    local pname
    pname=$(ps -p "$pid" -o comm= 2>/dev/null)
    if kill "$pid" 2>/dev/null; then
      print -P "%F{${_NAVE_C[ok]:-78}}✓ Finalizado PID ${pid} (${pname}) na porta ${port}.%f"
    else
      print -P "%F{${_NAVE_C[err]:-203}}× Nao foi possivel matar PID ${pid} (${pname}) — tente sudo kill -9 ${pid}.%f"
    fi
  done
}

# ------------------------------------------------------------------------------
# Project Hub: p [projeto] (multi-level project discovery up to depth 3)
# ------------------------------------------------------------------------------
function _nave_list_projects {
  local base="$1"
  [[ -d "$base" ]] || return 1
  local -a found=()
  local -A seen=()
  local d rel

  # 1. Top-level directories directly inside $NAVE_PROJECTS_DIR
  for d in "$base"/*(N/); do
    rel="${d#$base/}"
    seen[$rel]=1
    found+=("$rel")
  done

  # 2. Nested project roots (depth 2..3) identified by git/build markers
  local marker
  for marker in "$base"/*/*/{.git,Cargo.toml,package.json,pyproject.toml,CMakeLists.txt,Makefile}(N) \
                "$base"/*/*/*/{.git,Cargo.toml,package.json,pyproject.toml,CMakeLists.txt,Makefile}(N); do
    d="${marker:h}"
    rel="${d#$base/}"
    [[ "$rel" == *(node_modules|target|.venv|venv|dist|build)* ]] && continue
    if (( ! ${+seen[$rel]} )); then
      seen[$rel]=1
      found+=("$rel")
    fi
  done

  (( ${#found} )) && print -rl -- "${(@o)found}"
}

function p {
  local base="$NAVE_PROJECTS_DIR"
  if [[ ! -d "$base" ]]; then
    echo "Diretorio nao encontrado: $base (configure NAVE_PROJECTS_DIR no ~/.zshrc)"
    return 1
  fi

  local -a projects=("${(@f)$(_nave_list_projects "$base")}")
  local target="$1"
  if [[ -z "$target" ]]; then
    if command -v fzf >/dev/null 2>&1; then
      target=$(print -rl -- "${projects[@]}" | \
        fzf --header="Projetos em ${base} (ate nivel 3)" \
            --preview "dir='$base/{}'; echo '=== ' {} ' ==='; git -C \"\$dir\" status -sb 2>/dev/null; echo; git -C \"\$dir\" log --oneline -n 6 2>/dev/null; echo; eza --color=always --group-directories-first \"\$dir\" 2>/dev/null || ls -la \"\$dir\"")
      [[ -z "$target" ]] && return 0
    else
      print -rl -- "${projects[@]}"
      return 0
    fi
  elif [[ ! -d "$base/$target" ]]; then
    # Smart fuzzy lookup: exact basename -> prefix basename -> substring path (case-insensitive)
    local q="${target:l}" cand lbase lpath
    local m_exact="" m_pref="" m_sub=""
    for cand in "${projects[@]}"; do
      lpath="${cand:l}"
      lbase="${cand:t:l}"
      if [[ "$lbase" == "$q" ]]; then
        m_exact="$cand"; break
      elif [[ -z "$m_pref" && "$lbase" == ${q}* ]]; then
        m_pref="$cand"
      elif [[ -z "$m_sub" && "$lpath" == *${q}* ]]; then
        m_sub="$cand"
      fi
    done
    target="${m_exact:-${m_pref:-${m_sub:-$target}}}"
  fi

  if [[ -d "$base/$target" ]]; then
    cd "$base/$target" || return
    if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
      local br last_c
      br=$(git symbolic-ref --short HEAD 2>/dev/null || git rev-parse --short HEAD 2>/dev/null)
      last_c=$(git log -1 --pretty='%h %s (%cr)' 2>/dev/null)
      print -P "%F{${_NAVE_C[git]:-141}}± ${br}%f  %F{${_NAVE_C[muted]:-242}}${last_c}%f"
    fi
  else
    print -P "%F{${_NAVE_C[err]:-203}}Projeto nao encontrado:%f $base/$target"
    return 1
  fi
}

function _nave_comp_p {
  [[ -d "$NAVE_PROJECTS_DIR" ]] || return
  local -a projs=("${(@f)$(_nave_list_projects "$NAVE_PROJECTS_DIR")}")
  compadd -Q -a projs
}
compdef _nave_comp_p p 2>/dev/null

# ------------------------------------------------------------------------------
# Fuzzy File & Grep Jumpers integrated with nave-edit (ff / fg)
# ------------------------------------------------------------------------------
function ff {
  command -v fzf >/dev/null 2>&1 || { echo "ff requer fzf."; return 1; }
  local root="${1:-.}"
  local picked
  if command -v fd >/dev/null 2>&1; then
    picked=$(fd --type f --hidden --follow --exclude .git --exclude node_modules --exclude target . "$root" | \
      fzf --header="ff — Abrir arquivo no ne (Enter)" \
          --preview 'bat --color=always --style=numbers --line-range=:200 {} 2>/dev/null || head -n 200 {}')
  else
    picked=$(find "$root" -type f -not -path '*/.git/*' -not -path '*/node_modules/*' -not -path '*/target/*' | \
      fzf --header="ff — Abrir arquivo no ne (Enter)" \
          --preview 'bat --color=always --style=numbers --line-range=:200 {} 2>/dev/null || head -n 200 {}')
  fi
  [[ -n "$picked" ]] && "${EDITOR:-nave-edit}" "$picked"
}

function fg {
  if [[ "$1" == %* ]]; then
    builtin fg "$@"
    return
  fi
  command -v fzf >/dev/null 2>&1 && command -v rg >/dev/null 2>&1 || { echo "fg requer rg e fzf."; return 1; }
  local query="$*"
  local picked
  picked=$(rg --line-number --no-heading --color=always --smart-case --hidden \
    --glob '!.git/*' --glob '!node_modules/*' --glob '!target/*' "${query:-.}" 2>/dev/null | \
    fzf --ansi --delimiter : --query="$query" \
        --header="fg — Buscar conteudo e abrir na linha exata no ne (Enter)" \
        --preview 'bat --color=always --style=numbers --highlight-line {2} --line-range $(( {2} > 10 ? {2} - 10 : 1 )):$(( {2} + 40 )) {1} 2>/dev/null || sed -n "$(( {2} > 5 ? {2} - 5 : 1 )),$(( {2} + 20 ))p" {1}')
  [[ -n "$picked" ]] || return 0
  local file line
  file="${picked%%:*}"
  local rest="${picked#*:}"
  line="${rest%%:*}"
  "${EDITOR:-nave-edit}" "${file}:${line}"
}

# ------------------------------------------------------------------------------
# Smart Runner & Watcher: run [-d|--debug] [arquivo] [args...] / watchrun
# Auto-detects C++/C/Python/Rust/Node, injects in.txt, checks expected.txt, times in ms!
# ------------------------------------------------------------------------------
function _nave_run_detect_target {
  local cand
  for cand in sol.cpp main.cpp a.cpp main.py sol.py main.c main.rs; do
    [[ -f "$cand" ]] && { print -r -- "$cand"; return 0; }
  done
  [[ -f "Cargo.toml" ]] && { print -r -- "Cargo.toml"; return 0; }
  [[ -f "package.json" ]] && { print -r -- "package.json"; return 0; }
  [[ -f "Makefile" ]] && { print -r -- "Makefile"; return 0; }
  local -a srcs=(*.cpp(N) *.py(N) *.c(N) *.rs(N))
  if (( ${#srcs} == 1 )); then
    print -r -- "${srcs[1]}"
    return 0
  fi
  return 1
}

function _nave_run_exec_timed {
  local -a cmd=("$@")
  local in_file="" exp_file=""
  [[ -t 0 && -f "in.txt" ]] && in_file="in.txt"
  if [[ -f "expected.txt" ]]; then exp_file="expected.txt"
  elif [[ -f "ans.txt" ]]; then exp_file="ans.txt"
  fi

  local c_mut="${_NAVE_C[muted]:-244}" c_ok="${_NAVE_C[ok]:-48}" c_err="${_NAVE_C[err]:-197}" c_info="${_NAVE_C[info]:-51}"
  [[ -n "$in_file" ]] && print -P "%F{${c_mut}}  ↳ entrada automatica: ${in_file}%f"

  local t0=$EPOCHREALTIME rc=0 out_tmp=""
  if [[ -n "$exp_file" ]]; then
    out_tmp="/tmp/nave_run_out_$$.txt"
    if [[ -n "$in_file" ]]; then
      "${cmd[@]}" < "$in_file" | tee "$out_tmp"
      rc=${pipestatus[1]}
    else
      "${cmd[@]}" | tee "$out_tmp"
      rc=${pipestatus[1]}
    fi
  else
    if [[ -n "$in_file" ]]; then
      "${cmd[@]}" < "$in_file"
      rc=$?
    else
      "${cmd[@]}"
      rc=$?
    fi
  fi
  local t1=$EPOCHREALTIME
  local ms=$(( (t1 - t0) * 1000.0 ))

  if (( rc == 0 )); then
    printf "\e[38;5;%sm✓ exit 0\e[0m  \e[38;5;%sm%.1f ms\e[0m\n" "$c_ok" "$c_info" "$ms"
  else
    printf "\e[38;5;%sm× exit %d\e[0m  \e[38;5;%sm%.1f ms\e[0m\n" "$c_err" "$rc" "$c_info" "$ms"
  fi

  if [[ -n "$exp_file" && -f "$out_tmp" ]]; then
    if diff -q -Z "$out_tmp" "$exp_file" >/dev/null 2>&1; then
      print -P "%F{${c_ok}}%B✓ AC%b (match exato com ${exp_file})%f"
    else
      print -P "%F{${c_err}}%B× WA%b (diferenca em relacao a ${exp_file}):%f"
      if command -v delta >/dev/null 2>&1; then
        diff -u -Z "$exp_file" "$out_tmp" | delta --paging=never 2>/dev/null || diff -u --color=always -Z "$exp_file" "$out_tmp"
      else
        diff -u --color=always -Z "$exp_file" "$out_tmp"
      fi
    fi
    rm -f "$out_tmp"
  fi
  return $rc
}

function run {
  local debug_mode=0
  if [[ "$1" == "-d" || "$1" == "--debug" ]]; then
    debug_mode=1
    shift
  fi

  local target="$1"
  if [[ -z "$target" ]]; then
    target=$(_nave_run_detect_target) || {
      print -P "%F{${_NAVE_C[warn]:-214}}Uso: run [-d|--debug] <arquivo.cpp|.c|.py|.rs|.js|.sh> [args...]%f"
      return 1
    }
  else
    shift
  fi

  local c_dir="${_NAVE_C[dir]:-45}" c_warn="${_NAVE_C[warn]:-214}" c_err="${_NAVE_C[err]:-197}"
  local bin="/tmp/nave_run_$$"

  case "$target" in
    *.cpp|*.cc|*.cxx)
      local -a flags=(-O2 -std=c++20 -Wall -Wextra -DLOCAL)
      local tag="O2 · C++20 · -DLOCAL"
      if (( debug_mode )); then
        flags=(-O1 -g -std=c++20 -Wall -Wextra -DLOCAL -D_GLIBCXX_DEBUG -fsanitize=address,undefined)
        tag="DEBUG · ASan+UBSan"
      fi
      print -P "%F{${c_dir}}▸ g++ [${tag}] %B${target}%b%f"
      if g++ "${flags[@]}" "$target" -o "$bin"; then
        _nave_run_exec_timed "$bin" "$@"
        local rc=$?
        rm -f "$bin"
        return $rc
      else
        rm -f "$bin"
        print -P "%F{${c_err}}× Erro de compilacao em ${target}%f"
        return 1
      fi
      ;;
    *.c)
      local -a flags=(-O2 -std=c17 -Wall -Wextra -DLOCAL)
      (( debug_mode )) && flags=(-O1 -g -std=c17 -Wall -Wextra -DLOCAL -fsanitize=address,undefined)
      print -P "%F{${c_dir}}▸ gcc %B${target}%b%f"
      if gcc "${flags[@]}" "$target" -o "$bin"; then
        _nave_run_exec_timed "$bin" "$@"
        local rc=$?
        rm -f "$bin"
        return $rc
      else
        rm -f "$bin"
        return 1
      fi
      ;;
    *.py)
      print -P "%F{${c_dir}}▸ python3 %B${target}%b%f"
      _nave_run_exec_timed python3 "$target" "$@"
      ;;
    *.rs)
      print -P "%F{${c_dir}}▸ rustc %B${target}%b%f"
      if rustc -O "$target" -o "$bin"; then
        _nave_run_exec_timed "$bin" "$@"
        local rc=$?
        rm -f "$bin"
        return $rc
      else
        rm -f "$bin"
        return 1
      fi
      ;;
    Cargo.toml)
      print -P "%F{${c_dir}}▸ cargo run%f"
      cargo run "$@"
      ;;
    package.json)
      local pm="npm"
      [[ -f "pnpm-lock.yaml" ]] && command -v pnpm >/dev/null 2>&1 && pm="pnpm"
      local script="dev"
      grep -q '"dev"' package.json 2>/dev/null || script="start"
      print -P "%F{${c_dir}}▸ ${pm} run ${script}%f"
      "$pm" run "$script" "$@"
      ;;
    Makefile)
      print -P "%F{${c_dir}}▸ make%f"
      make "$@"
      ;;
    *.js|*.mjs)
      print -P "%F{${c_dir}}▸ node %B${target}%b%f"
      _nave_run_exec_timed node "$target" "$@"
      ;;
    *.sh|*.zsh)
      print -P "%F{${c_dir}}▸ ${target:e} %B${target}%b%f"
      _nave_run_exec_timed "${target:e}" "$target" "$@"
      ;;
    *)
      if [[ -x "$target" ]]; then
        _nave_run_exec_timed "./$target" "$@"
      else
        print -P "%F{${c_warn}}Formato nao reconhecido para run: ${target}%f"
        return 1
      fi
      ;;
  esac
}

function watchrun {
  local debug_flag=""
  if [[ "$1" == "-d" || "$1" == "--debug" ]]; then
    debug_flag="$1"
    shift
  fi
  local target="$1"
  [[ -n "$target" ]] || target=$(_nave_run_detect_target) || {
    echo "Uso: watchrun [-d|--debug] <arquivo> [args...]"
    return 1
  }
  [[ "$1" == "$target" ]] && shift
  [[ -e "$target" ]] || { echo "Arquivo nao encontrado: $target"; return 1; }

  local last_mt="" cur_mt=""
  trap 'return 0' INT
  while true; do
    cur_mt=$(stat -c %Y "$target" 2>/dev/null)
    [[ -f "in.txt" ]] && cur_mt+=":$(stat -c %Y in.txt 2>/dev/null)"
    if [[ "$cur_mt" != "$last_mt" ]]; then
      last_mt="$cur_mt"
      clear
      hr "watchrun: ${target}"
      if [[ -n "$debug_flag" ]]; then
        run "$debug_flag" "$target" "$@"
      else
        run "$target" "$@"
      fi
      print -P "%F{${_NAVE_C[muted]:-244}}  [observando ${target} · salve para re-executar · Ctrl+C sair]%f"
    fi
    sleep 0.35
  done
}

# ------------------------------------------------------------------------------
# Git Turbo (ginfo, gbf, gwip, gunwip, gbclean)
# ------------------------------------------------------------------------------
function ginfo {
  git rev-parse --is-inside-work-tree >/dev/null 2>&1 || { echo "Nao e um repositorio Git."; return 1; }
  local repo br remote stashes changed
  repo=$(basename "$(git rev-parse --show-toplevel)")
  br=$(git symbolic-ref --short HEAD 2>/dev/null || git rev-parse --short HEAD)
  remote=$(git remote get-url origin 2>/dev/null || echo "sem remote")
  stashes=$(git stash list 2>/dev/null | wc -l | tr -d ' ')
  changed=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')

  print -P "%F{${_NAVE_C[dir]:-39}}╭─ Git: %B${repo}%b ────────────────────────────────────%f"
  print -P "%F{${_NAVE_C[frame]:-240}}│%f %F{${_NAVE_C[git]:-141}}Branch:%f   ${br}  %F{${_NAVE_C[muted]:-242}}(alterados: ${changed}, stashes: ${stashes})%f"
  print -P "%F{${_NAVE_C[frame]:-240}}│%f %F{${_NAVE_C[git]:-141}}Remote:%f   ${remote}"
  print -P "%F{${_NAVE_C[frame]:-240}}├───────────────────────────────────────────────────%f"
  git log --color=always --pretty="format:  %C(yellow)%h%C(reset) %s %C(dim)(%cr, %an)%C(reset)" -n 6
  echo ""
  if (( changed > 0 )); then
    print -P "%F{${_NAVE_C[frame]:-240}}├───────────────────────────────────────────────────%f"
    git -c color.status=always status -sb | sed 's/^/  /'
  fi
  print -P "%F{${_NAVE_C[dir]:-39}}╰───────────────────────────────────────────────────%f"
}

function gbf {
  git rev-parse --is-inside-work-tree >/dev/null 2>&1 || { echo "Nao e um repositorio Git."; return 1; }
  local br
  br=$(git branch --all --sort=-committerdate --format='%(refname:short)' | \
    fzf --header="Trocar branch" \
        --preview "git log --oneline --graph --color=always -n 15 {}")
  [[ -z "$br" ]] && return 0
  git switch "${br#origin/}"
}

unalias gwip gunwip 2>/dev/null

function gwip {
  git add -A && git commit -m "wip: work in progress [$(date +%H:%M)]" --no-verify
}

function gunwip {
  local last_msg
  last_msg=$(git log -1 --pretty=%B 2>/dev/null)
  if [[ "$last_msg" == wip:* ]]; then
    git reset HEAD~1
    print -P "%F{${_NAVE_C[ok]:-78}}✓ Commit WIP desfeito (alteracoes preservadas no working tree).%f"
  else
    print -P "%F{${_NAVE_C[warn]:-215}}Ultimo commit nao e um WIP:%f ${last_msg}"
    return 1
  fi
}

function gbclean {
  git rev-parse --is-inside-work-tree >/dev/null 2>&1 || return 1
  local main_br
  main_br=$(git symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null | sed 's|^origin/||')
  [[ -z "$main_br" ]] && main_br="main"
  local merged
  merged=$(git branch --merged "$main_br" | grep -vE "^\*|^\s*(main|master|develop)$")
  if [[ -z "$merged" ]]; then
    print -P "%F{${_NAVE_C[ok]:-78}}Nenhuma branch local mergeada para limpar.%f"
    return 0
  fi
  print -P "%F{${_NAVE_C[warn]:-215}}Removendo branches ja mergeadas em ${main_br}:%f"
  echo "$merged" | xargs -r git branch -d
}

function glogf {
  git rev-parse --is-inside-work-tree >/dev/null 2>&1 || { echo "Nao e um repositorio Git."; return 1; }
  command -v fzf >/dev/null 2>&1 || { git log --oneline --graph -n 25; return 0; }
  local prev_cmd='sha=$(echo {} | grep -oE "[a-f0-9]{7,}" | head -n1); [ -n "$sha" ] && git show --color=always "$sha" | (delta --paging=never 2>/dev/null || cat)'
  local picked
  picked=$(git log --graph --color=always --format="%C(yellow)%h%C(reset) %s %C(dim)(%cr · %an)%C(reset)%C(auto)%d%C(reset)" -n 200 | \
    fzf --ansi --no-sort --reverse \
        --header="glogf — Enter abre commit · Ctrl+Y copia SHA" \
        --preview "$prev_cmd" --preview-window="right:60%" \
        --expect="ctrl-y")
  [[ -n "$picked" ]] || return 0
  local key="${picked%%$'\n'*}"
  local line="${picked#*$'\n'}"
  local sha
  sha=$(grep -oE '[a-f0-9]{7,}' <<< "$line" | head -n 1)
  [[ -n "$sha" ]] || return 0
  if [[ "$key" == "ctrl-y" ]]; then
    if command -v clip.exe >/dev/null 2>&1; then printf '%s' "$sha" | clip.exe; fi
    print -P "%F{${_NAVE_C[ok]:-48}}✓ SHA copiado:%f %B${sha}%b"
  else
    git show "$sha"
  fi
}

function gdiff {
  git rev-parse --is-inside-work-tree >/dev/null 2>&1 || { echo "Nao e um repositorio Git."; return 1; }
  command -v fzf >/dev/null 2>&1 || { git diff; return 0; }
  local picked
  picked=$(git status --short | \
    fzf --header="gdiff — Selecione arquivo modificado (Enter abre no ne)" \
        --preview 'f=$(echo {} | awk "{print \$NF}"); git diff --color=always -- "$f" | (delta --paging=never 2>/dev/null || cat)' \
        --preview-window="right:65%")
  [[ -n "$picked" ]] || return 0
  local file
  file=$(awk '{print $NF}' <<< "$picked")
  [[ -n "$file" ]] && "${EDITOR:-nave-edit}" "$file"
}

# ------------------------------------------------------------------------------
# Docker Cockpit (dks, dkl, dksh)
# ------------------------------------------------------------------------------
function dks {
  command -v docker >/dev/null 2>&1 || { echo "Docker nao encontrado."; return 1; }
  print -P "%F{${_NAVE_C[docker]:-75}}╭─ Docker Containers ─────────────────────────────────────────%f"
  docker ps -a --format "table {{.Names}}\t{{.Status}}\t{{.Image}}\t{{.Ports}}" 2>/dev/null || \
    print -P "%F{${_NAVE_C[warn]:-215}}  Docker daemon nao esta respondendo.%f"
  print -P "%F{${_NAVE_C[docker]:-75}}╰─────────────────────────────────────────────────────────────%f"
}

function dkl {
  local name="$1"
  if [[ -z "$name" ]] && command -v fzf >/dev/null 2>&1; then
    name=$(docker ps -a --format '{{.Names}}\t{{.Status}}' | fzf --header="Ver logs do container" | awk '{print $1}')
  fi
  [[ -z "$name" ]] && return 0
  docker logs -f --tail=100 "$name"
}

function dksh {
  local name="$1"
  if [[ -z "$name" ]] && command -v fzf >/dev/null 2>&1; then
    name=$(docker ps --format '{{.Names}}\t{{.Image}}' | fzf --header="Abrir shell no container" | awk '{print $1}')
  fi
  [[ -z "$name" ]] && return 0
  docker exec -it "$name" bash 2>/dev/null || docker exec -it "$name" sh
}

# ------------------------------------------------------------------------------
# Visual Cockpit / sysinfo v3 (TrueColor Gradient Space Art + Progress Bars)
# ------------------------------------------------------------------------------
function _nave_bar {
  local pct="${1:-0}"
  local width="${2:-16}"
  (( pct < 0 )) && pct=0
  (( pct > 100 )) && pct=100
  local filled=$(( (pct * width + 50) / 100 ))
  local empty=$(( width - filled ))
  local color="${_NAVE_C[ok]:-78}"
  (( pct >= 70 )) && color="${_NAVE_C[warn]:-215}"
  (( pct >= 88 )) && color="${_NAVE_C[err]:-203}"
  local bar_f="" bar_e=""
  (( filled > 0 )) && bar_f="${(l:${filled}::█:)}"
  (( empty > 0 )) && bar_e="${(l:${empty}::░:)}"
  echo -n "%F{${color}}${bar_f}%f%F{${_NAVE_C[frame]:-240}}${bar_e}%f"
}

# Fast live Windows host RAM reader for WSL (~40ms via native GlobalMemoryStatusEx helper)
function _nave_win_mem {
  local -a helper=(/mnt/c/Users/*/.config/nave/nave-winmem.exe(N[1]))
  if [[ -n "${helper[1]}" && -x "${helper[1]}" ]]; then
    "${helper[1]}" </dev/null 2>/dev/null | tr -d '\r'
    return 0
  fi
  return 1
}

function sysinfo {
  local os kernel up mem_used mem_tot mem_pct wsl_used wsl_tot wsl_pct disk_used disk_tot disk_pct_f disk_pct_i ip_addr open_ports dk_run
  os=$(grep -m1 PRETTY_NAME /etc/os-release 2>/dev/null | cut -d= -f2 | tr -d '"')
  kernel=$(uname -r)
  up=$(uptime -p 2>/dev/null | sed 's/^up //')

  local disk_target="/" disk_lbl="Disk /:"
  if [[ -d /mnt/c ]] && df -k /mnt/c >/dev/null 2>&1; then
    disk_target="/mnt/c"
    disk_lbl="SSD C::"
  fi
  read -r wsl_used wsl_tot wsl_pct <<< "$(free -m | awk '/^Mem:/ {printf "%.1f %.1f %d", $3/1024, $2/1024, int(($3/$2)*100 + 0.5)}')"
  local win_mem mem_line
  win_mem=$(_nave_win_mem)
  if [[ -n "$win_mem" ]]; then
    read -r mem_used mem_tot mem_pct <<< "$win_mem"
    mem_line="$(_nave_bar "$mem_pct" 14) ${mem_used}/${mem_tot}Gi (${mem_pct}%%) %F{${_NAVE_C[muted]:-244}}· WSL ${wsl_pct}%%%f"
  else
    mem_used="$wsl_used"; mem_tot="$wsl_tot"; mem_pct="$wsl_pct"
    mem_line="$(_nave_bar "$mem_pct" 14) ${mem_used}/${mem_tot}Gi (${mem_pct}%%)"
  fi
  read -r disk_used disk_tot disk_pct_f disk_pct_i <<< "$(df -k "$disk_target" | awk 'NR==2 {u=$3/1048576; t=$2/1048576; p=(u/t)*100; printf "%.1fG %.1fG %.1f %d", u, t, p, int(p+0.5)}')"
  ip_addr=$(hostname -I 2>/dev/null | awk '{print $1}')
  open_ports=$(ss -tlnH 2>/dev/null | wc -l | tr -d ' ')
  dk_run=$(docker ps -q 2>/dev/null | wc -l | tr -d ' ')

  local disk_bar
  disk_bar=$(_nave_bar "$disk_pct_i" 14)

  local c_dir="${_NAVE_C[dir]:-39}"
  local c_frm="${_NAVE_C[frame]:-240}"
  local c_lbl="${_NAVE_C[git]:-141}"

  local -a art
  art=(
    "         ▲         "
    "        ╱█╲        "
    "       ╱███╲       "
    "      ╱█ ◆ █╲      "
    "     ╱███████╲     "
    "    ╱██ ▀█▀ ██╲    "
    "   ╰──╯  ║  ╰──╯   "
    "    N · A · V · E  "
  )

  local -a info
  info=(
    "%F{${c_dir}}╭─ %B${USER}@${HOST}%b %F{${c_frm}}[${NAVE_PALETTE} · ${NAVE_LAYOUT}]%f"
    "%F{${c_frm}}│%f %F{${c_lbl}}OS:%f      ${os:-Linux}"
    "%F{${c_frm}}│%f %F{${c_lbl}}Kernel:%f  ${kernel}"
    "%F{${c_frm}}│%f %F{${c_lbl}}Uptime:%f  ${up:-N/A}"
    "%F{${c_frm}}│%f %F{${c_lbl}}Memory:%f  ${mem_line}"
    "%F{${c_frm}}│%f %F{${c_lbl}}${disk_lbl}%f  ${disk_bar} ${disk_used}/${disk_tot} (${disk_pct_f}%%)"
    "%F{${c_frm}}│%f %F{${c_lbl}}Net:%f     WSL ${ip_addr:-N/A} · ${open_ports} TCP · ${dk_run} docker"
    "%F{${c_dir}}╰──────────────────────────────────────────────%f"
  )

  local i
  for (( i = 1; i <= ${#info}; i++ )); do
    if (( ${COLUMNS:-100} >= 76 )); then
      _nave_grad_line "${art[$i]}" 1
      printf "  "
    fi
    print -P "${info[$i]}"
  done
}

function nave-welcome {
  case "$1" in
    on|1)  print -r -- "1" > "$NAVE_CONFIG_DIR/welcome"; print -P "%F{${_NAVE_C[ok]:-78}}Welcome banner ativado.%f" ;;
    off|0) print -r -- "0" > "$NAVE_CONFIG_DIR/welcome"; print -P "%F{${_NAVE_C[warn]:-215}}Welcome banner desativado.%f" ;;
    *)
      local st="0"
      [[ -r "$NAVE_CONFIG_DIR/welcome" ]] && st="$(<"$NAVE_CONFIG_DIR/welcome")"
      print -P "Welcome banner: %B$([[ $st == 1 ]] && echo on || echo off)%b (uso: nave-welcome on|off)"
      ;;
  esac
}

function _nave_show_welcome {
  local enabled="0"
  [[ -r "$NAVE_CONFIG_DIR/welcome" ]] && enabled="$(<"$NAVE_CONFIG_DIR/welcome")"
  [[ "$enabled" == "1" ]] || return 0
  [[ -o interactive && -t 1 ]] || return 0

  local mem_pct disk_pct_f disk_pct_i mem_bar disk_bar disk_target="/" win_mem _u _t
  [[ -d /mnt/c ]] && df -k /mnt/c >/dev/null 2>&1 && disk_target="/mnt/c"
  win_mem=$(_nave_win_mem)
  if [[ -n "$win_mem" ]]; then
    read -r _u _t mem_pct <<< "$win_mem"
  else
    mem_pct=$(free -m | awk '/^Mem:/ {printf "%d", int(($3/$2)*100 + 0.5)}')
  fi
  read -r disk_pct_f disk_pct_i <<< "$(df -k "$disk_target" | awk 'NR==2 {p=($3/$2)*100; printf "%.1f %d", p, int(p+0.5)}')"
  mem_bar=$(_nave_bar "$mem_pct" 10)
  disk_bar=$(_nave_bar "$disk_pct_i" 10)

  print -P "%F{${_NAVE_C[frame]:-240}}╭─%f %F{${_NAVE_C[dir]:-39}}%Bnave%b%f  RAM ${mem_bar} ${mem_pct}%%  ·  SSD ${disk_bar} ${disk_pct_f}%%  ·  %F{${_NAVE_C[muted]:-242}}digite 'nave-help' para atalhos%f"
}

function nave-doctor {
  local c_dir="${_NAVE_C[dir]:-45}" c_frm="${_NAVE_C[frame]:-240}" c_ok="${_NAVE_C[ok]:-48}" c_warn="${_NAVE_C[warn]:-214}" c_err="${_NAVE_C[err]:-197}" c_mut="${_NAVE_C[muted]:-244}"

  print -P "%F{${c_dir}}╭─ Nave Terminal v4 — Diagnostico (nave-doctor) ──────────────%f"

  # 1. Startup speed
  local t0=$EPOCHREALTIME
  zsh -i -c 'exit 0' >/dev/null 2>&1
  local t1=$EPOCHREALTIME
  local start_ms=$(( (t1 - t0) * 1000.0 ))
  local st_col="$c_ok"
  (( start_ms > 120 )) && st_col="$c_warn"
  (( start_ms > 250 )) && st_col="$c_err"
  printf "\e[38;5;%sm│\e[0m  Startup Zsh:   \e[38;5;%sm%.1f ms\e[0m  (skip_global_compinit=%s)\n" \
    "$c_frm" "$st_col" "$start_ms" "${skip_global_compinit:-off}"

  # 2. History health
  local h_col="$c_ok"
  (( HISTSIZE < 10000 || SAVEHIST < 10000 )) && h_col="$c_err"
  print -P "%F{${c_frm}}│%f  Historico:     %F{${h_col}}HISTSIZE=${HISTSIZE} · SAVEHIST=${SAVEHIST} · ${#history} carregados%f"

  # 3. Bytecode cache (.zwc)
  local zwc_cnt=0
  local -a zwcs=("${NAVE_CONFIG_DIR:-$HOME/.config/nave}"/*.zwc(N))
  zwc_cnt=${#zwcs}
  print -P "%F{${c_frm}}│%f  Modulos .zwc:  %F{${c_ok}}${zwc_cnt} compilados%f  ·  Paleta: %B${NAVE_PALETTE}%b · Layout: %B${NAVE_LAYOUT}%b"

  # 4. Core binaries & Win32 helpers
  local ne_st="%F{${c_err}}ausente%f"
  command -v nave-edit >/dev/null 2>&1 && ne_st="%F{${c_ok}}ok ($(nave-edit --version 2>/dev/null | awk '{print $2}'))%f"
  local wm_st="%F{${c_warn}}ausente%f" hm_st="%F{${c_warn}}ausente%f"
  local -a wmh=(/mnt/c/Users/*/.config/nave/nave-winmem.exe(N[1]))
  local -a hmh=(/mnt/c/Users/*/.config/nave/nave-hidemouse.exe(N[1]))
  [[ -n "${wmh[1]}" ]] && wm_st="%F{${c_ok}}ok%f"
  [[ -n "${hmh[1]}" ]] && hm_st="%F{${c_ok}}ok%f"
  print -P "%F{${c_frm}}│%f  Binarios:      ne=${ne_st}  ·  winmem=${wm_st}  ·  hidemouse=${hm_st}"

  # 5. CLI enhancers
  local cli_line="" tool
  for tool in fzf fd rg bat eza delta lazygit btop; do
    if command -v "$tool" >/dev/null 2>&1; then
      cli_line+="%F{${c_ok}}✓${tool}%f "
    else
      cli_line+="%F{${c_mut}}·${tool}%f "
    fi
  done
  print -P "%F{${c_frm}}│%f  CLI Extras:    ${cli_line}"

  # 6. Windows Terminal settings.json validation
  local -a wt_arr=(/mnt/c/Users/*/AppData/Local/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json(N[1]))
  if [[ -n "${wt_arr[1]}" && -f "${wt_arr[1]}" ]]; then
    local wt_res
    wt_res=$(python3 - "${wt_arr[1]}" <<'PY' 2>&1
import json, sys
try:
    txt = open(sys.argv[1], encoding="utf-8").read()
    data = json.loads(txt)
    keys = {kb.get("keys","") for kb in data.get("keybindings", [])}
    want = ["ctrl+sc(41)", "ctrl+sc(86)", "ctrl+sc(115)"]
    missing = [w for w in want if w not in keys]
    if missing:
        print("WARN:" + ",".join(missing))
    else:
        print("OK")
except Exception as e:
    print("ERR:" + str(e))
PY
)
    if [[ "$wt_res" == "OK" ]]; then
      print -rP "%F{${c_frm}}│%f  Win Terminal:  %F{${c_ok}}JSON valido · atalhos ABNT2 (Ctrl+' / \\ / /) ativos%f"
    elif [[ "$wt_res" == WARN:* ]]; then
      print -P "%F{${c_frm}}│%f  Win Terminal:  %F{${c_warn}}JSON valido · faltam bindings: ${wt_res#WARN:}%f"
    else
      print -P "%F{${c_frm}}│%f  Win Terminal:  %F{${c_err}}${wt_res}%f"
    fi
  fi

  print -P "%F{${c_dir}}╰─────────────────────────────────────────────────────────────%f"
}

function nave-help {
  local c_dir="${_NAVE_C[dir]:-39}"
  local c_frm="${_NAVE_C[frame]:-240}"
  local c_ok="${_NAVE_C[ok]:-78}"
  local c_lbl="${_NAVE_C[git]:-141}"

  print -P "%F{${c_dir}}╭─ Nave Terminal v4 — Guia Rapido ──────────────────────────%f"
  print -P "%F{${c_frm}}│%f %F{${c_lbl}}%BEstilo, Arte & Cockpit%b%f"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}nave-theme <nome>%f      Paleta + ne sync (cyber|emerald|sunset|nord|stealth|dracula)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}nave-layout <modo>%f     Layout do prompt (framed|rail|hud|compact)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}nave-shader <modo>%f     Shader GPU Windows Terminal (glow|crt|off)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}nave-glass [modo]%f      Alterna fundo translucido/solido (off|on|wall|40..100)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}nave-bg / transient%f    Liga/desliga fundo OSC 11 ou transient prompt"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}sysinfo / hr [titulo]%f  Arte + barras de sistema / regua divisoria"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}nave-hud / nave-warp%f   Cockpit tempo real ([1-6],[p]) / starfield 3D"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}ne <arquivo> (Ctrl+')%f  Editor nativo Rust (multi-painel + terminal integrado)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}nave-doctor%f            Diagnostico completo (startup, historico, helpers, WT)"
  print -P "%F{${c_frm}}│%f"
  print -P "%F{${c_frm}}│%f %F{${c_lbl}}%BNavegacao, Busca & Execucao%b%f"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}p [projeto]%f            Pula p/ projetos ate nivel 3 em \$NAVE_PROJECTS_DIR"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}ff / fg [termo]%f        Busca fuzzy de arquivos/conteudo abrindo direto no ne"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}run [-d] / watchrun%f    Compila/roda (C++/Py/Rust/Node, in.txt, expected.txt)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}z <termo> / zi / z<Tab>%f Salto inteligente nativo (nave-z frecency)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}Ctrl+R / Ctrl+T / Alt+C%f Busca fuzzy no historico / arquivos / pastas"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}nave-suggest / highlight%f Sugestoes v2 (Alt+↓/↑ alterna) e cores v2 nativas"
  print -P "%F{${c_frm}}│%f"
  print -P "%F{${c_frm}}│%f %F{${c_lbl}}%BEdicao Estilo VS Code%b%f"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}Shift+Setas / Ctrl+A/L%f Seleciona texto/linha (digitar/colar substitui)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}Ctrl+D / Ctrl+Space%f    Seleciona palavra (2x expande) / Autocomplete"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}Ctrl+←/→ / Ctrl+BS/Del%f Navega e apaga por subpalavra/pasta"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}Ctrl+Z / Ctrl+Y / +ShZ%f Desfazer e Refazer na linha de comando"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}Ctrl+C / Ctrl+X / +V%f   Cancela (ou copia selecao) / Recorta / Cola"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}Sh+Alt+↓ / Ctrl+Sh+K%f   Duplica / Apaga linha · Ctrl+/ comenta · Sh+Enter"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}Ctrl+F / Alt+L / Alt+E%f Busca na aba / Limpa tela / Abre no \$EDITOR"
  print -P "%F{${c_frm}}│%f"
  print -P "%F{${c_frm}}│%f %F{${c_lbl}}%BGit, Docker, Portas & WSL%b%f"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}lg / ginfo / gbf%f       Lazygit, painel do repo, trocar branch fuzzy"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}glogf / gdiff%f          Explorer interativo de commits e diff (delta -> ne)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}gwip / gunwip / gbclean%f Commit WIP rapido e limpeza de branches"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}dks / dkl / dksh%f       Containers, logs fuzzy e shell fuzzy"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}ports / killport [p]%f   Lista portas abertas e mata porta (interativo c/ fzf)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}clip / paste / winopen%f Clipboard Windows e abrir no Explorer"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}notify <cmd> / mon%f     Toast ao terminar comando / monitor btop"
  print -P "%F{${c_dir}}╰───────────────────────────────────────────────────────────%f"
}

# Backward compatibility if toolkit.zsh is sourced standalone without init.zsh
if (( ! _NAVE_INIT_ACTIVE )); then
  [[ -r "$HOME/.config/nave/keybindings.zsh" ]] && source "$HOME/.config/nave/keybindings.zsh"
  _nave_show_welcome
fi
