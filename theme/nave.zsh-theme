# ==============================================================================
# ~/.config/nave/theme.zsh
# Nave Zsh Theme v4 — Standalone, palette-driven, 100% universal (zero Nerd Fonts)
# Features:
#   - 6 Obsidian Vibrant + Dracula palettes + live OSC 11/12 & nave-edit sync
#   - 4 prompt layouts: framed | rail | hud | compact (nave-layout <mode>)
#   - Transient prompt (collapses old prompts to 1 compact status-aware line)
#   - Windows native toast notifications for long commands (>= 15s by default)
#   - Smart directory truncation, rich Git status, 0ms dev context (venv/node/rust/go/py/c++/docker)
# ==============================================================================

autoload -Uz add-zsh-hook add-zle-hook-widget
zmodload zsh/datetime 2>/dev/null

typeset -g NAVE_CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/nave"
[[ -d "$NAVE_CONFIG_DIR" ]] || mkdir -p "$NAVE_CONFIG_DIR"

typeset -g _NAVE_CMD_START=0
typeset -g _NAVE_CMD_STR=""
typeset -g _NAVE_CMD_DURATION=""
typeset -g _NAVE_FIRST_PROMPT=1
typeset -gi _NAVE_LAST_EXIT=0
typeset -g _NAVE_CACHE_NODE_PATH=""
typeset -g _NAVE_CACHE_NODE_VER=""
typeset -gA _NAVE_C

# Load persisted settings
typeset -g NAVE_PALETTE="cyber"
[[ -r "$NAVE_CONFIG_DIR/palette" ]] && NAVE_PALETTE="$(<"$NAVE_CONFIG_DIR/palette")"

typeset -g NAVE_LAYOUT="framed"
[[ -r "$NAVE_CONFIG_DIR/layout" ]] && NAVE_LAYOUT="$(<"$NAVE_CONFIG_DIR/layout")"

typeset -g _NAVE_TRANSIENT=1
[[ -r "$NAVE_CONFIG_DIR/transient" ]] && _NAVE_TRANSIENT="$(<"$NAVE_CONFIG_DIR/transient")"

typeset -g _NAVE_BG_SYNC=1
[[ -r "$NAVE_CONFIG_DIR/bg_sync" ]] && _NAVE_BG_SYNC="$(<"$NAVE_CONFIG_DIR/bg_sync")"

typeset -g NAVE_NOTIFY_THRESHOLD=15
[[ -r "$NAVE_CONFIG_DIR/notify" ]] && NAVE_NOTIFY_THRESHOLD="$(<"$NAVE_CONFIG_DIR/notify")"

# ------------------------------------------------------------------------------
# Palette Engine (256-color + TrueColor gradients + live OSC 11/12 bg/cursor)
# ------------------------------------------------------------------------------
function _nave_apply_palette() {
  local pal="${1:-$NAVE_PALETTE}"
  case "$pal" in
    cyber)
      _NAVE_C=(
        frame 240  user 51   host 135  dir 45    git 171
        warn 214   info 51   ok 48     err 197   dur 220
        muted 244  node 83   docker 39 path 87   str 220
        bg "#08090F" cursor "#00F0FF"
        grad1 "0;245;255" grad2 "124;77;255" grad3 "255;46;151"
        fzf_hl "#00f0ff" fzf_bg "#131524" fzf_ptr "#a855f7" fzf_info "#ff2e97" fzf_border "#3b2f63"
      )
      ;;
    emerald)
      _NAVE_C=(
        frame 239  user 48   host 51   dir 46    git 118
        warn 226   info 51   ok 82     err 197   dur 190
        muted 244  node 84   docker 45 path 85   str 226
        bg "#070C09" cursor "#00FF88"
        grad1 "0;255;136" grad2 "0;229;255" grad3 "178;255;89"
        fzf_hl "#00ff88" fzf_bg "#0e1d14" fzf_ptr "#00e5ff" fzf_info "#b2ff59" fzf_border "#1f593b"
      )
      ;;
    sunset)
      _NAVE_C=(
        frame 240  user 208  host 204  dir 197   git 220
        warn 226   info 214  ok 48     err 196   dur 226
        muted 244  node 118  docker 45 path 215  str 220
        bg "#0C070A" cursor "#FF5E3A"
        grad1 "255;214;0" grad2 "255;94;58" grad3 "255;30;142"
        fzf_hl "#ff5e3a" fzf_bg "#1e1018" fzf_ptr "#ff1e8e" fzf_info "#ffd600" fzf_border "#5c2540"
      )
      ;;
    nord)
      _NAVE_C=(
        frame 240  user 87   host 75   dir 45    git 141
        warn 221   info 87   ok 85     err 203   dur 220
        muted 244  node 84   docker 39 path 123  str 221
        bg "#080B12" cursor "#38E8FF"
        grad1 "56;232;255" grad2 "108;140;255" grad3 "192;132;252"
        fzf_hl "#38e8ff" fzf_bg "#121a2b" fzf_ptr "#6c8cff" fzf_info "#c084fc" fzf_border "#2c3f66"
      )
      ;;
    stealth)
      _NAVE_C=(
        frame 239  user 255  host 141  dir 135   git 177
        warn 220   info 141  ok 255    err 197   dur 183
        muted 243  node 141  docker 135 path 255 str 213
        bg "#08080B" cursor "#A855F7"
        grad1 "248;250;252" grad2 "168;85;247" grad3 "236;72;153"
        fzf_hl "#c084fc" fzf_bg "#15121f" fzf_ptr "#a855f7" fzf_info "#ec4899" fzf_border "#4c2875"
      )
      ;;
    dracula)
      _NAVE_C=(
        frame 239  user 212  host 183  dir 117   git 183
        warn 228   info 117  ok 84     err 210   dur 228
        muted 103  node 84   docker 117 path 159 str 228
        bg "#10111A" cursor "#FF73C6"
        grad1 "255;115;198" grad2 "184;130;255" grad3 "107;227;245"
        fzf_hl "#ff73c6" fzf_bg "#1c1f2e" fzf_ptr "#b882ff" fzf_info "#6be3f5" fzf_border "#3b2a5e"
      )
      ;;
    *)
      print -P "%F{203}Paleta desconhecida: ${pal}%f (opcoes: cyber, emerald, sunset, nord, stealth, dracula)"
      return 1
      ;;
  esac

  NAVE_PALETTE="$pal"
  typeset -g _NAVE_HL_LAST_PAL=""

  # Live terminal background & cursor color sync (OSC 11 & OSC 12)
  if (( _NAVE_BG_SYNC )) && [[ -o interactive && -t 1 && "$TERM" != "dumb" ]]; then
    printf '\e]11;%s\a\e]12;%s\a' "${_NAVE_C[bg]}" "${_NAVE_C[cursor]}"
  fi

  # Sync optional fzf color palette
  export FZF_DEFAULT_OPTS="--height=60% --layout=reverse --border=rounded --margin=0,1 --info=inline-right --prompt='❯ ' --pointer='›' --marker='*' --color=hl:${_NAVE_C[fzf_hl]},hl+:${_NAVE_C[fzf_hl]},bg+:${_NAVE_C[fzf_bg]},pointer:${_NAVE_C[fzf_ptr]},prompt:${_NAVE_C[fzf_ptr]},info:${_NAVE_C[fzf_info]},border:${_NAVE_C[fzf_border]}"
}

function _nave_sync_edit_palette() {
  local pal="$1"
  local conf="$NAVE_CONFIG_DIR/edit.conf"
  [[ -f "$conf" ]] || return 0
  local tmp="${conf}.tmp.$$"
  if grep -q '^palette=' "$conf" 2>/dev/null; then
    sed "s/^palette=.*/palette=${pal}/" "$conf" >| "$tmp" && command mv -f "$tmp" "$conf"
  else
    { cat "$conf"; print -r -- "palette=${pal}"; } >| "$tmp" && command mv -f "$tmp" "$conf"
  fi
}

function nave-theme() {
  if [[ -z "$1" || "$1" == "list" ]]; then
    print -P "%F{${_NAVE_C[dir]}}╭─ Paletas disponiveis (atual: %B${NAVE_PALETTE}%b)%f"
    for p in cyber emerald sunset nord stealth dracula; do
      local mark="  "
      [[ "$p" == "$NAVE_PALETTE" ]] && mark="❯ "
      print -P "%F{${_NAVE_C[frame]}}│%f ${mark}%B${p}%b"
    done
    print -P "%F{${_NAVE_C[dir]}}╰─ Uso: nave-theme <nome>%f"
    return 0
  fi
  if _nave_apply_palette "$1"; then
    print -r -- "$1" >| "$NAVE_CONFIG_DIR/palette"
    _nave_sync_edit_palette "$1"
    print -P "%F{${_NAVE_C[ok]}}✓ Paleta alterada para %B${1}%b%f %F{${_NAVE_C[muted]}}(bg ${_NAVE_C[bg]} · ne sincronizado)%f"
  fi
}

function nave-layout() {
  if [[ -z "$1" || "$1" == "list" ]]; then
    print -P "%F{${_NAVE_C[dir]}}╭─ Layouts disponiveis (atual: %B${NAVE_LAYOUT}%b)%f"
    for l in framed rail hud compact; do
      local mark="  "
      [[ "$l" == "$NAVE_LAYOUT" ]] && mark="❯ "
      print -P "%F{${_NAVE_C[frame]}}│%f ${mark}%B${l}%b"
    done
    print -P "%F{${_NAVE_C[dir]}}╰─ Uso: nave-layout <framed|rail|hud|compact>%f"
    return 0
  fi
  case "$1" in
    framed|rail|hud|compact)
      NAVE_LAYOUT="$1"
      print -r -- "$1" > "$NAVE_CONFIG_DIR/layout"
      print -P "%F{${_NAVE_C[ok]}}✓ Layout alterado para %B${1}%b%f"
      ;;
    *)
      print -P "%F{203}Layout desconhecido: $1%f (opcoes: framed, rail, hud, compact)"
      return 1
      ;;
  esac
}

function nave-bg() {
  case "$1" in
    on|1)
      _NAVE_BG_SYNC=1
      print -r -- "1" > "$NAVE_CONFIG_DIR/bg_sync"
      _nave_apply_palette "$NAVE_PALETTE"
      print -P "%F{${_NAVE_C[ok]}}Sincronizacao de fundo/cursor (OSC 11/12) ativada.%f"
      ;;
    off|0)
      _NAVE_BG_SYNC=0
      print -r -- "0" > "$NAVE_CONFIG_DIR/bg_sync"
      [[ -t 1 ]] && printf '\e]111\a\e]112\a'
      print -P "%F{${_NAVE_C[warn]}}Sincronizacao de fundo/cursor desativada (padrao do terminal restaurado).%f"
      ;;
    *)
      local st="on"
      (( _NAVE_BG_SYNC )) || st="off"
      print -P "Background sync (OSC 11/12): %B${st}%b (uso: nave-bg on|off)"
      ;;
  esac
}

function nave-transient() {
  case "$1" in
    on|1)
      _NAVE_TRANSIENT=1
      print -r -- "1" > "$NAVE_CONFIG_DIR/transient"
      print -P "%F{${_NAVE_C[ok]}}Transient prompt ativado.%f"
      ;;
    off|0)
      _NAVE_TRANSIENT=0
      print -r -- "0" > "$NAVE_CONFIG_DIR/transient"
      print -P "%F{${_NAVE_C[warn]}}Transient prompt desativado.%f"
      ;;
    *)
      local st="on"
      (( _NAVE_TRANSIENT )) || st="off"
      print -P "Transient prompt: %B${st}%b (uso: nave-transient on|off)"
      ;;
  esac
}

function nave-notify() {
  if [[ "$1" == "off" || "$1" == "0" ]]; then
    NAVE_NOTIFY_THRESHOLD=0
    print -r -- "0" > "$NAVE_CONFIG_DIR/notify"
    print -P "%F{${_NAVE_C[warn]}}Notificacoes automaticas desativadas.%f"
  elif [[ "$1" =~ '^[0-9]+$' ]]; then
    NAVE_NOTIFY_THRESHOLD="$1"
    print -r -- "$1" > "$NAVE_CONFIG_DIR/notify"
    print -P "%F{${_NAVE_C[ok]}}Notificacao automatica para comandos >= ${1}s.%f"
  else
    print -P "Threshold atual: %B${NAVE_NOTIFY_THRESHOLD}s%b (uso: nave-notify <segundos|off>)"
  fi
}

# ------------------------------------------------------------------------------
# Windows Native Toast Notification (async, zero blocking)
# ------------------------------------------------------------------------------
function _nave_win_toast() {
  command -v powershell.exe >/dev/null 2>&1 || return
  local title="$1"
  local body="$2"
  title="${title//&/&amp;}"; title="${title//</&lt;}"; title="${title//>/&gt;}"; title="${title//\"/&quot;}"; title="${title//\'/&apos;}"
  body="${body//&/&amp;}"; body="${body//</&lt;}"; body="${body//>/&gt;}"; body="${body//\"/&quot;}"; body="${body//\'/&apos;}"

  local ps_script="
    [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > \$null
    [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] > \$null
    \$xml = New-Object Windows.Data.Xml.Dom.XmlDocument
    \$xml.LoadXml('<toast><visual><binding template=\"ToastText02\"><text id=\"1\">${title}</text><text id=\"2\">${body}</text></binding></visual></toast>')
    \$toast = [Windows.UI.Notifications.ToastNotification]::new(\$xml)
    \$appId = '{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe'
    [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier(\$appId).Show(\$toast)
  "
  (powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -Command "$ps_script" </dev/null >/dev/null 2>&1 &)
}

# ------------------------------------------------------------------------------
# Hooks & Segments
# ------------------------------------------------------------------------------
function _nave_preexec() {
  _NAVE_CMD_START=$EPOCHSECONDS
  _NAVE_CMD_STR="$1"
}

function _nave_format_duration() {
  local secs=$1
  local out=""
  if (( secs >= 86400 )); then out+="$((secs / 86400))d"; secs=$((secs % 86400)); fi
  if (( secs >= 3600 ));  then out+="$((secs / 3600))h";  secs=$((secs % 3600));  fi
  if (( secs >= 60 ));    then out+="$((secs / 60))m";    secs=$((secs % 60));    fi
  if (( secs > 0 || ${#out} > 0 )); then
    [[ -n "$out" || $secs -ge 2 ]] && out+="${secs}s"
  fi
  printf '%s' "$out"
}

function _nave_git_info() {
  local git_dir
  git_dir=$(command git rev-parse --git-dir 2>/dev/null) || return

  local branch
  branch=$(command git symbolic-ref --short HEAD 2>/dev/null)
  if [[ -z "$branch" ]]; then
    branch=$(command git rev-parse --short HEAD 2>/dev/null)
    [[ -n "$branch" ]] && branch="@${branch}"
  fi
  [[ -z "$branch" ]] && return

  local action=""
  if [[ -d "${git_dir}/rebase-merge" || -d "${git_dir}/rebase-apply" ]]; then
    action="REBASE"
  elif [[ -f "${git_dir}/MERGE_HEAD" ]]; then
    action="MERGE"
  elif [[ -f "${git_dir}/CHERRY_PICK_HEAD" ]]; then
    action="CHERRY-PICK"
  fi

  local dirty="" staged="" untracked="" ahead_behind=""
  local git_status
  git_status=$(command git status --porcelain=v1 --branch 2>/dev/null)

  local first_line="${git_status%%$'\n'*}"
  if [[ "$first_line" =~ 'ahead ([0-9]+)' ]]; then ahead_behind+="↑${match[1]}"; fi
  if [[ "$first_line" =~ 'behind ([0-9]+)' ]]; then ahead_behind+="↓${match[1]}"; fi

  local line
  while IFS= read -r line; do
    [[ "$line" == "## "* ]] && continue
    local x="${line[1]}" y="${line[2]}"
    if [[ "$x" == "?" ]]; then
      untracked="?"
    else
      [[ "$x" != " " ]] && staged="+"
      [[ "$y" != " " ]] && dirty="*"
    fi
    [[ -n "$staged" && -n "$dirty" && -n "$untracked" ]] && break
  done <<< "$git_status"

  local stash_count
  stash_count=$(command git rev-list --walk-reflogs --count refs/stash 2>/dev/null)
  local stash_part=""
  [[ -n "$stash_count" && "$stash_count" -gt 0 ]] && stash_part=" ≡${stash_count}"

  local flags="${staged}${dirty}${untracked}"
  local out="%F{${_NAVE_C[git]}}± ${branch}%f"
  [[ -n "$action" ]] && out+=" %F{${_NAVE_C[err]}}%B${action}%b%f"
  [[ -n "$flags" ]] && out+=" %F{${_NAVE_C[warn]}}${flags}%f"
  [[ -n "$ahead_behind" ]] && out+=" %F{${_NAVE_C[info]}}${ahead_behind}%f"
  [[ -n "$stash_part" ]] && out+=" %F{${_NAVE_C[muted]}}${stash_part}%f"

  printf '%s' "$out"
}

function _nave_context_info() {
  local ctx=""
  local has_lang=0

  if [[ -n "$NAVE_INTEGRATED" ]]; then
    ctx+=" %F{${_NAVE_C[info]}}[ne]%f"
  fi

  if [[ -n "$VIRTUAL_ENV" ]]; then
    ctx+=" %F{${_NAVE_C[ok]}}venv:${VIRTUAL_ENV:t}%f"
    has_lang=1
  elif [[ -n "$CONDA_DEFAULT_ENV" ]]; then
    ctx+=" %F{${_NAVE_C[ok]}}conda:${CONDA_DEFAULT_ENV}%f"
    has_lang=1
  elif [[ -f "pyproject.toml" || -f "requirements.txt" ]]; then
    ctx+=" %F{${_NAVE_C[ok]}}py%f"
    has_lang=1
  fi

  if [[ -f "Cargo.toml" ]]; then
    ctx+=" %F{${_NAVE_C[warn]}}rust%f"
    has_lang=1
  fi

  if [[ -f "go.mod" ]]; then
    ctx+=" %F{${_NAVE_C[info]}}go%f"
    has_lang=1
  fi

  if [[ -f "package.json" ]]; then
    if [[ "$_NAVE_CACHE_NODE_PATH" != "$PATH" ]]; then
      _NAVE_CACHE_NODE_PATH="$PATH"
      if command -v node >/dev/null 2>&1; then
        _NAVE_CACHE_NODE_VER="$(node -v 2>/dev/null)"
      else
        _NAVE_CACHE_NODE_VER=""
      fi
    fi
    [[ -n "$_NAVE_CACHE_NODE_VER" ]] && ctx+=" %F{${_NAVE_C[node]}}node:${_NAVE_CACHE_NODE_VER}%f"
    has_lang=1
  fi

  if (( ! has_lang )) && [[ -f "CMakeLists.txt" || -f "Makefile" ]]; then
    ctx+=" %F{${_NAVE_C[path]}}c/c++%f"
  fi

  if [[ -f "Dockerfile" || -f "docker-compose.yml" || -f "docker-compose.yaml" || -f "compose.yaml" ]]; then
    ctx+=" %F{${_NAVE_C[docker]}}[docker]%f"
  fi

  printf '%s' "${ctx# }"
}

function _nave_visible_len() {
  setopt LOCAL_OPTIONS EXTENDED_GLOB
  local expanded
  expanded=$(print -P -- "$1")
  expanded="${expanded//$'\e'\[[0-9;]#m/}"
  printf '%s' "${#expanded}"
}

function _nave_precmd() {
  local last_exit=$?
  _NAVE_LAST_EXIT=$last_exit

  if (( _NAVE_FIRST_PROMPT )); then
    _NAVE_FIRST_PROMPT=0
  else
    [[ "$NAVE_LAYOUT" != "compact" ]] && print ""
  fi

  _NAVE_CMD_DURATION=""
  if (( _NAVE_CMD_START > 0 )); then
    local elapsed=$(( EPOCHSECONDS - _NAVE_CMD_START ))
    _NAVE_CMD_DURATION=$(_nave_format_duration "$elapsed")
    local cmd_first="${_NAVE_CMD_STR%% *}"
    if (( NAVE_NOTIFY_THRESHOLD > 0 && elapsed >= NAVE_NOTIFY_THRESHOLD )); then
      case "$cmd_first" in
        vim|nvim|nano|ne|nave-edit|less|man|top|htop|btop|lazygit|lg|ssh|tmux|python|python3|node|ipython|claude|codex|gemini|grok|nave-hud|nave-warp|watchrun) ;;
        *)
          local status_lbl="Concluido (${_NAVE_CMD_DURATION})"
          (( last_exit != 0 )) && status_lbl="Falhou [exit ${last_exit}] (${_NAVE_CMD_DURATION})"
          _nave_win_toast "WSL nave — ${status_lbl}" "${_NAVE_CMD_STR:0:90}"
          ;;
      esac
    fi
    _NAVE_CMD_START=0
    _NAVE_CMD_STR=""
  fi

  local user_color="%F{${_NAVE_C[user]}}"
  [[ $EUID -eq 0 ]] && user_color="%F{${_NAVE_C[err]}}"

  local ssh_badge=""
  [[ -n "$SSH_CONNECTION" ]] && ssh_badge="%F{${_NAVE_C[warn]}}[ssh]%f "

  local user_host="${ssh_badge}${user_color}%n%f%F{${_NAVE_C[muted]}}@%f%F{${_NAVE_C[host]}}%m%f"
  local dir_part="%F{${_NAVE_C[dir]}}%B%(4~|…/%3~|%~)%b%f"

  local git_part ctx_part dur_part="" err_part=""
  git_part=$(_nave_git_info)
  ctx_part=$(_nave_context_info)
  [[ -n "$_NAVE_CMD_DURATION" ]] && dur_part="%F{${_NAVE_C[dur]}}${_NAVE_CMD_DURATION}%f"
  (( last_exit != 0 )) && err_part="%F{${_NAVE_C[err]}}%B×${last_exit}%b%f"

  local arrow_color="%F{${_NAVE_C[ok]}}"
  (( last_exit != 0 )) && arrow_color="%F{${_NAVE_C[err]}}"

  print -Pn "\e]0;%n@%m: %~\a"

  case "$NAVE_LAYOUT" in
    compact)
      local segs="${user_host} ${dir_part}"
      [[ -n "$git_part" ]] && segs+=" ${git_part}"
      [[ -n "$err_part" ]] && segs+=" ${err_part}"
      PROMPT="${segs} ${arrow_color}%B❯%b%f "
      RPROMPT="%(1j.%F{${_NAVE_C[warn]}}[%j bg]%f .)%F{${_NAVE_C[muted]}}%D{%H:%M}%f"
      ;;
    hud)
      local sep=" %F{${_NAVE_C[frame]}}│%f "
      local line1="%F{${_NAVE_C[frame]}}╭─%f %F{${_NAVE_C[dir]}}◆%f ${user_host}${sep}${dir_part}"
      [[ -n "$git_part" ]] && line1+="${sep}${git_part}"
      [[ -n "$ctx_part" ]] && line1+="${sep}${ctx_part}"
      [[ -n "$dur_part" ]] && line1+="${sep}${dur_part}"
      [[ -n "$err_part" ]] && line1+="${sep}${err_part}"
      PROMPT="${line1}
%F{${_NAVE_C[frame]}}╰─%f ${arrow_color}%B❯%b%f "
      RPROMPT="%(1j.%F{${_NAVE_C[warn]}}[%j bg]%f .)%F{${_NAVE_C[muted]}}[%D{%H:%M:%S}]%f"
      ;;
    rail)
      local left="%F{${_NAVE_C[frame]}}╭─%f ${user_host}  ${dir_part}"
      [[ -n "$git_part" ]] && left+="  ${git_part}"
      [[ -n "$ctx_part" ]] && left+="  ${ctx_part}"
      [[ -n "$dur_part" ]] && left+="  ${dur_part}"
      [[ -n "$err_part" ]] && left+="  ${err_part}"
      local right="%F{${_NAVE_C[muted]}}[%D{%H:%M:%S}]%f"
      local l_len r_len fill_len rail_str="" cols=${COLUMNS:-0}
      (( cols < 20 )) && cols=100
      l_len=$(_nave_visible_len "$left")
      r_len=$(_nave_visible_len "$right")
      fill_len=$(( cols - l_len - r_len - 2 ))
      if (( fill_len >= 3 )); then
        rail_str=" %F{${_NAVE_C[frame]}}${(l:${fill_len}::─:)}%f ${right}"
      else
        rail_str="  ${right}"
      fi
      PROMPT="${left}${rail_str}
%F{${_NAVE_C[frame]}}╰─%f ${arrow_color}%B❯%b%f "
      RPROMPT="%(1j.%F{${_NAVE_C[warn]}}[%j bg]%f .)"
      ;;
    *) # framed (default)
      local line1="%F{${_NAVE_C[frame]}}╭─%f ${user_host}  ${dir_part}"
      [[ -n "$git_part" ]] && line1+="  ${git_part}"
      [[ -n "$ctx_part" ]] && line1+="  ${ctx_part}"
      [[ -n "$dur_part" ]] && line1+="  ${dur_part}"
      [[ -n "$err_part" ]] && line1+="  ${err_part}"
      PROMPT="${line1}
%F{${_NAVE_C[frame]}}╰─%f ${arrow_color}%B❯%b%f "
      RPROMPT="%(1j.%F{${_NAVE_C[warn]}}[%j bg]%f .)%F{${_NAVE_C[muted]}}[%D{%H:%M:%S}]%f"
      ;;
  esac
}

function _nave_transient_prompt() {
  (( _NAVE_TRANSIENT )) || return
  local arr_c="${_NAVE_C[ok]}"
  (( _NAVE_LAST_EXIT != 0 )) && arr_c="${_NAVE_C[err]}"
  PROMPT="%F{${_NAVE_C[muted]}}%D{%H:%M}%f %F{${arr_c}}❯%f "
  RPROMPT=""
  zle .reset-prompt
}

_nave_apply_palette "$NAVE_PALETTE"

add-zsh-hook preexec _nave_preexec
add-zsh-hook precmd _nave_precmd
add-zle-hook-widget zle-line-finish _nave_transient_prompt
