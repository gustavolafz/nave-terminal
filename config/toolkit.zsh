# ==============================================================================
# ~/.config/nave/toolkit.zsh
# Modular WSL + Dev + Modern CLI + Art/Cockpit toolkit
# ==============================================================================

: "${NAVE_PROJECTS_DIR:=$HOME/projects}"

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

  source <(fzf --zsh)
fi

if command -v zoxide >/dev/null 2>&1; then
  eval "$(zoxide init zsh)"
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
  if [[ -z "$1" ]]; then
    echo "Uso: killport <porta>"
    return 1
  fi
  local port="$1"
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
# Project Hub: p [projeto]
# ------------------------------------------------------------------------------
function p {
  local base="$NAVE_PROJECTS_DIR"
  if [[ ! -d "$base" ]]; then
    echo "Diretorio nao encontrado: $base (configure NAVE_PROJECTS_DIR no ~/.zshrc)"
    return 1
  fi

  local target="$1"
  if [[ -z "$target" ]]; then
    if command -v fzf >/dev/null 2>&1; then
      target=$(find "$base" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort | \
        fzf --header="Projetos em ${base}" \
            --preview "dir='$base/{}'; echo '=== ' {} ' ==='; git -C \"\$dir\" status -sb 2>/dev/null; echo; git -C \"\$dir\" log --oneline -n 6 2>/dev/null; echo; eza --color=always --group-directories-first \"\$dir\" 2>/dev/null || ls -la \"\$dir\"")
      [[ -z "$target" ]] && return 0
    else
      ls "$base"
      return 0
    fi
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
  [[ -d "$NAVE_PROJECTS_DIR" ]] && _files -W "$NAVE_PROJECTS_DIR" -/
}
compdef _nave_comp_p p 2>/dev/null

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

function sysinfo {
  local os kernel up mem_used mem_tot mem_pct disk_used disk_tot disk_pct ip_addr open_ports dk_run
  os=$(grep -m1 PRETTY_NAME /etc/os-release 2>/dev/null | cut -d= -f2 | tr -d '"')
  kernel=$(uname -r)
  up=$(uptime -p 2>/dev/null | sed 's/^up //')

  local disk_target="/" disk_lbl="Disk /:"
  if [[ -d /mnt/c ]] && df -h /mnt/c >/dev/null 2>&1; then
    disk_target="/mnt/c"
    disk_lbl="SSD C::"
  fi
  read -r mem_used mem_tot mem_pct <<< "$(free -m | awk '/^Mem:/ {printf "%.1fGi %.1fGi %d", $3/1024, $2/1024, ($3/$2)*100}')"
  read -r disk_used disk_tot disk_pct <<< "$(df -h "$disk_target" | awk 'NR==2 {gsub(/%/,"",$5); print $3, $2, $5}')"
  ip_addr=$(hostname -I 2>/dev/null | awk '{print $1}')
  open_ports=$(ss -tlnH 2>/dev/null | wc -l | tr -d ' ')
  dk_run=$(docker ps -q 2>/dev/null | wc -l | tr -d ' ')

  local mem_bar disk_bar
  mem_bar=$(_nave_bar "$mem_pct" 14)
  disk_bar=$(_nave_bar "$disk_pct" 14)

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
    "%F{${c_frm}}│%f %F{${c_lbl}}Memory:%f  ${mem_bar} ${mem_used}/${mem_tot} (${mem_pct}%%)"
    "%F{${c_frm}}│%f %F{${c_lbl}}${disk_lbl}%f  ${disk_bar} ${disk_used}/${disk_tot} (${disk_pct}%%)"
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
      local st="1"
      [[ -r "$NAVE_CONFIG_DIR/welcome" ]] && st="$(<"$NAVE_CONFIG_DIR/welcome")"
      print -P "Welcome banner: %B$([[ $st == 1 ]] && echo on || echo off)%b (uso: nave-welcome on|off)"
      ;;
  esac
}

function _nave_show_welcome {
  local enabled="1"
  [[ -r "$NAVE_CONFIG_DIR/welcome" ]] && enabled="$(<"$NAVE_CONFIG_DIR/welcome")"
  [[ "$enabled" == "1" ]] || return
  [[ -o interactive && -t 1 ]] || return

  local mem_pct disk_pct mem_bar disk_bar disk_target="/"
  [[ -d /mnt/c ]] && df -h /mnt/c >/dev/null 2>&1 && disk_target="/mnt/c"
  mem_pct=$(free -m | awk '/^Mem:/ {printf "%d", ($3/$2)*100}')
  disk_pct=$(df -h "$disk_target" | awk 'NR==2 {gsub(/%/,"",$5); print $5}')
  mem_bar=$(_nave_bar "$mem_pct" 10)
  disk_bar=$(_nave_bar "$disk_pct" 10)

  print -P "%F{${_NAVE_C[frame]:-240}}╭─%f %F{${_NAVE_C[dir]:-39}}%Bnave%b%f  RAM ${mem_bar} ${mem_pct}%%  ·  SSD ${disk_bar} ${disk_pct}%%  ·  %F{${_NAVE_C[muted]:-242}}digite 'nave-help' para atalhos%f"
}

function nave-help {
  local c_dir="${_NAVE_C[dir]:-39}"
  local c_frm="${_NAVE_C[frame]:-240}"
  local c_ok="${_NAVE_C[ok]:-78}"
  local c_lbl="${_NAVE_C[git]:-141}"

  print -P "%F{${c_dir}}╭─ Nave Terminal — Guia Rapido ─────────────────────────────%f"
  print -P "%F{${c_frm}}│%f %F{${c_lbl}}%BEstilo, Arte & Cockpit%b%f"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}nave-theme <nome>%f      Paleta + fundo ao vivo (cyber|emerald|sunset|nord|stealth)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}nave-layout <modo>%f     Layout do prompt (framed|rail|hud|compact)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}nave-shader <modo>%f     Shader GPU Windows Terminal (glow|crt|off)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}nave-bg / transient%f    Liga/desliga fundo OSC 11 ou transient prompt"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}sysinfo / hr [titulo]%f  Arte + barras de sistema / regua divisoria"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}nave-hud / nave-warp%f   Cockpit em tempo real / starfield 3D"
  print -P "%F{${c_frm}}│%f"
  print -P "%F{${c_frm}}│%f %F{${c_lbl}}%BNavegacao & Busca%b%f"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}p [projeto]%f            Pula para \$NAVE_PROJECTS_DIR (fzf + preview)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}z <termo> / zi%f         Salto inteligente (zoxide)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}Ctrl+R / Ctrl+T / Alt+C%f Busca fuzzy no historico / arquivos / pastas"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}Esc Esc / ll / lt / b%f  Toggle sudo / eza Git / arvore / bat viewer"
  print -P "%F{${c_frm}}│%f"
  print -P "%F{${c_frm}}│%f %F{${c_lbl}}%BEdicao Estilo VS Code%b%f"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}Shift+Setas / Ctrl+A%f   Seleciona texto (digitar/colar/BS substitui)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}Ctrl+←/→ / Ctrl+BS/Del%f Navega e apaga por subpalavra/pasta"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}Ctrl+Z / Ctrl+Y / +ShZ%f Desfazer e Refazer na linha de comando"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}Ctrl+C / Ctrl+X / +V%f   Copia selecao / Recorta / Cola (Windows)"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}Ctrl+Sh+K / D / Ctrl+/%f Apaga linha / duplica / comenta (#) · Alt+E editor"
  print -P "%F{${c_frm}}│%f"
  print -P "%F{${c_frm}}│%f %F{${c_lbl}}%BGit, Docker, Portas & WSL%b%f"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}lg / ginfo / gbf%f       Lazygit, painel do repo, trocar branch fuzzy"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}gwip / gunwip / gbclean%f Commit WIP rapido e limpeza de branches"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}dks / dkl / dksh%f       Containers, logs fuzzy e shell fuzzy"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}ports / killport <p>%f   Lista portas abertas e mata processo na porta"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}clip / paste / winopen%f Clipboard Windows e abrir no Explorer"
  print -P "%F{${c_frm}}│%f   %F{${c_ok}}notify <cmd> / mon%f     Toast ao terminar comando / monitor btop"
  print -P "%F{${c_dir}}╰───────────────────────────────────────────────────────────%f"
}

# Load VS Code-style ZLE keybindings (selection, subword nav, undo/redo, clipboard)
[[ -r "$HOME/.config/nave/keybindings.zsh" ]] && source "$HOME/.config/nave/keybindings.zsh"

_nave_show_welcome
