# ==============================================================================
# Nave Zsh Theme v3 - Clean, fast, palette-driven, 100% universal across WSL
# Features:
#   - Live palette switcher + live terminal background/cursor sync (OSC 11 / 12)
#   - 4 prompt layouts: framed | rail | hud | compact (nave-layout <mode>)
#   - Transient prompt (collapses old prompts to 1 compact line after Enter)
#   - Windows native toast notifications for long commands (>= 15s by default)
#   - Smart directory truncation, rich Git status, venv/node/docker context
# ==============================================================================

autoload -Uz add-zsh-hook add-zle-hook-widget

typeset -g NAVE_CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/nave"
[[ -d "$NAVE_CONFIG_DIR" ]] || mkdir -p "$NAVE_CONFIG_DIR"

typeset -g _NAVE_CMD_START=0
typeset -g _NAVE_CMD_STR=""
typeset -g _NAVE_CMD_DURATION=""
typeset -g _NAVE_FIRST_PROMPT=1
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
        frame 240  user 81   host 111  dir 39    git 141
        warn 215   info 81   ok 78     err 203   dur 221
        muted 242  node 114  docker 75 path 117  str 221
        bg "#0E111A" cursor "#5CE1E6"
        grad1 "125;207;255" grad2 "122;162;247" grad3 "187;154;247"
        fzf_hl "#7dcfff" fzf_bg "#1a1b26" fzf_ptr "#7aa2f7" fzf_info "#bb9af7" fzf_border "#3b4261"
      )
      ;;
    emerald)
      _NAVE_C=(
        frame 239  user 48   host 79   dir 42    git 114
        warn 221   info 50   ok 82     err 203   dur 185
        muted 242  node 119  docker 44 path 85   str 192
        bg "#0B1510" cursor "#50FA7B"
        grad1 "80;250;123" grad2 "46;229;157" grad3 "56;189;248"
        fzf_hl "#50fa7b" fzf_bg "#0f1914" fzf_ptr "#2ee59d" fzf_info "#8be9fd" fzf_border "#28503c"
      )
      ;;
    sunset)
      _NAVE_C=(
        frame 240  user 215  host 209  dir 204   git 177
        warn 221   info 216  ok 114    err 196   dur 228
        muted 243  node 150  docker 111 path 217 str 222
        bg "#181016" cursor "#FF9E64"
        grad1 "255;215;95" grad2 "255;158;100" grad3 "247;118;142"
        fzf_hl "#ff9e64" fzf_bg "#1c1419" fzf_ptr "#f7768e" fzf_info "#e0af68" fzf_border "#523644"
      )
      ;;
    nord)
      _NAVE_C=(
        frame 240  user 110  host 109  dir 116   git 146
        warn 222   info 110  ok 108    err 174   dur 223
        muted 243  node 108  docker 110 path 152 str 223
        bg "#232934" cursor "#88C0D0"
        grad1 "136;192;208" grad2 "129;161;193" grad3 "180;142;173"
        fzf_hl "#88c0d0" fzf_bg "#2e3440" fzf_ptr "#81a1c1" fzf_info "#b48ead" fzf_border "#4c566a"
      )
      ;;
    stealth)
      _NAVE_C=(
        frame 238  user 250  host 245  dir 255   git 248
        warn 221   info 250  ok 252    err 203   dur 246
        muted 240  node 247  docker 247 path 252 str 249
        bg "#101012" cursor "#F4F4F5"
        grad1 "248;250;252" grad2 "180;188;200" grad3 "113;113;122"
        fzf_hl "#e5e9f0" fzf_bg "#121212" fzf_ptr "#d8dee9" fzf_info "#888888" fzf_border "#3a3a3a"
      )
      ;;
    *)
      print -P "%F{203}Paleta desconhecida: ${pal}%f (opcoes: cyber, emerald, sunset, nord, stealth)"
      return 1
      ;;
  esac

  NAVE_PALETTE="$pal"

  # Live terminal background & cursor color sync (OSC 11 & OSC 12)
  if (( _NAVE_BG_SYNC )) && [[ -o interactive && -t 1 && "$TERM" != "dumb" ]]; then
    printf '\e]11;%s\a\e]12;%s\a' "${_NAVE_C[bg]}" "${_NAVE_C[cursor]}"
  fi

  # Sync zsh-syntax-highlighting styles if loaded
  if (( ${+ZSH_HIGHLIGHT_STYLES} )); then
    ZSH_HIGHLIGHT_STYLES[command]="fg=${_NAVE_C[ok]},bold"
    ZSH_HIGHLIGHT_STYLES[builtin]="fg=${_NAVE_C[user]},bold"
    ZSH_HIGHLIGHT_STYLES[alias]="fg=${_NAVE_C[user]},bold"
    ZSH_HIGHLIGHT_STYLES[function]="fg=${_NAVE_C[user]},bold"
    ZSH_HIGHLIGHT_STYLES[unknown-token]="fg=${_NAVE_C[err]},bold"
    ZSH_HIGHLIGHT_STYLES[path]="fg=${_NAVE_C[path]},underline"
    ZSH_HIGHLIGHT_STYLES[single-quoted-argument]="fg=${_NAVE_C[str]}"
    ZSH_HIGHLIGHT_STYLES[double-quoted-argument]="fg=${_NAVE_C[str]}"
    ZSH_HIGHLIGHT_STYLES[dollar-double-quoted-argument]="fg=${_NAVE_C[git]}"
  fi

  # Sync fzf color palette
  export FZF_DEFAULT_OPTS="--height=60% --layout=reverse --border=rounded --margin=0,1 --info=inline-right --prompt='❯ ' --pointer='›' --marker='*' --color=hl:${_NAVE_C[fzf_hl]},hl+:${_NAVE_C[fzf_hl]},bg+:${_NAVE_C[fzf_bg]},pointer:${_NAVE_C[fzf_ptr]},prompt:${_NAVE_C[fzf_ptr]},info:${_NAVE_C[fzf_info]},border:${_NAVE_C[fzf_border]}"
}

function nave-theme() {
  if [[ -z "$1" || "$1" == "list" ]]; then
    print -P "%F{${_NAVE_C[dir]}}╭─ Paletas disponiveis (atual: %B${NAVE_PALETTE}%b)%f"
    for p in cyber emerald sunset nord stealth; do
      local mark="  "
      [[ "$p" == "$NAVE_PALETTE" ]] && mark="❯ "
      print -P "%F{${_NAVE_C[frame]}}│%f ${mark}%B${p}%b"
    done
    print -P "%F{${_NAVE_C[dir]}}╰─ Uso: nave-theme <nome>%f"
    return 0
  fi
  if _nave_apply_palette "$1"; then
    print -r -- "$1" > "$NAVE_CONFIG_DIR/palette"
    print -P "%F{${_NAVE_C[ok]}}✓ Paleta alterada para %B${1}%b%f %F{${_NAVE_C[muted]}}(bg ${_NAVE_C[bg]})%f"
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
  (powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -Command "$ps_script" >/dev/null 2>&1 &)
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
  echo -n "$out"
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

  echo -n "$out"
}

function _nave_context_info() {
  local ctx=""
  if [[ -n "$VIRTUAL_ENV" ]]; then
    ctx+=" %F{${_NAVE_C[ok]}}venv:${VIRTUAL_ENV:t}%f"
  elif [[ -n "$CONDA_DEFAULT_ENV" ]]; then
    ctx+=" %F{${_NAVE_C[ok]}}conda:${CONDA_DEFAULT_ENV}%f"
  fi

  if [[ -f "package.json" ]] && command -v node >/dev/null 2>&1; then
    local nver
    nver=$(node -v 2>/dev/null)
    [[ -n "$nver" ]] && ctx+=" %F{${_NAVE_C[node]}}node:${nver}%f"
  fi

  if [[ -f "Dockerfile" || -f "docker-compose.yml" || -f "docker-compose.yaml" || -f "compose.yaml" ]]; then
    ctx+=" %F{${_NAVE_C[docker]}}[docker]%f"
  fi

  echo -n "${ctx# }"
}

function _nave_visible_len() {
  local expanded
  expanded=$(print -P -- "$1")
  # Strip ANSI escape sequences
  expanded="${expanded//$'\e'\[[0-9;]#m/}"
  echo -n "${#expanded}"
}

function _nave_precmd() {
  local last_exit=$?

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
        vim|nvim|nano|less|man|top|htop|btop|lazygit|lg|ssh|tmux|python|python3|node|ipython|claude|codex|gemini|grok|nave-hud|nave-warp) ;;
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
  PROMPT="%F{${_NAVE_C[muted]}}%D{%H:%M}%f %F{${_NAVE_C[ok]}}❯%f "
  RPROMPT=""
  zle .reset-prompt
}

_nave_apply_palette "$NAVE_PALETTE"

add-zsh-hook preexec _nave_preexec
add-zsh-hook precmd _nave_precmd
add-zle-hook-widget zle-line-finish _nave_transient_prompt
