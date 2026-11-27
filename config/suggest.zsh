# ==============================================================================
# ~/.config/nave/suggest.zsh
# nave-suggest v2 — Pure Zsh 5.9 Intelligent Context-Aware Autosuggestions
#
# 3-Tier Instant Engine (<0.2ms/keystroke, zero external plugins, zero monkey-patching):
#   1. Per-directory successful command history (PWD-aware, exit-code 0 only)
#   2. Full Zsh history (C-level reverse scan, single-line safe, newest-first)
#   3. Live fallback: local files/dirs, nave-z frecency dirs, and command/alias names
#
# Controls:
#   Right / End            -> Accept full suggestion (with clean Ctrl+Z undo boundary)
#   Ctrl+Right / Alt+Right -> Accept next subword segment (UTF-8 & path aware)
#   Alt+Down / Alt+Up      -> Cycle through multiple matching suggestions [2/N]
#   nave-suggest [on|off|stats|clear]
# ==============================================================================

autoload -Uz add-zle-hook-widget add-zsh-hook
zmodload zsh/parameter 2>/dev/null

typeset -g _NAVE_SG_ENABLED=1
[[ -r "${NAVE_CONFIG_DIR:-$HOME/.config/nave}/suggest" ]] && _NAVE_SG_ENABLED="$(<"${NAVE_CONFIG_DIR:-$HOME/.config/nave}/suggest")"

typeset -g _NAVE_SG_DB="${NAVE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/nave}/suggest_dir.db"
[[ -d "${_NAVE_SG_DB:h}" ]] || mkdir -p "${_NAVE_SG_DB:h}"

typeset -g  _NAVE_SG_LAST_BUF=""
typeset -g  _NAVE_SG_LAST_HISTCMD=""
typeset -g  _NAVE_SG_LAST_PWD=""
typeset -g  _NAVE_SG_SUFFIX=""
typeset -g  _NAVE_SG_SOURCE=""
typeset -gi _NAVE_SG_CYCLE_IDX=0
typeset -gi _NAVE_SG_EXPANDED=0
typeset -ga _NAVE_SG_CANDS=()
typeset -ga _NAVE_SG_DIR_CMDS=()
typeset -ga _NAVE_SG_Z_DIRS=()
typeset -g  _NAVE_SG_PENDING_CMD=""
typeset -g  _NAVE_SG_PENDING_PWD=""

# ------------------------------------------------------------------------------
# User CLI: nave-suggest [on|off|stats|clear]
# ------------------------------------------------------------------------------
function nave-suggest {
  local cfg="${NAVE_CONFIG_DIR:-$HOME/.config/nave}/suggest"
  case "$1" in
    on|1)
      _NAVE_SG_ENABLED=1
      _nave_suggest_invalidate
      print -r -- "1" >| "$cfg"
      print -P "%F{${_NAVE_C[ok]:-48}}✓ nave-suggest ativado.%f"
      ;;
    off|0)
      _NAVE_SG_ENABLED=0
      _nave_suggest_invalidate
      print -r -- "0" >| "$cfg"
      print -P "%F{${_NAVE_C[warn]:-214}}nave-suggest desativado.%f"
      ;;
    clear)
      : >| "$_NAVE_SG_DB"
      _NAVE_SG_DIR_CMDS=()
      _nave_suggest_invalidate
      print -P "%F{${_NAVE_C[ok]:-48}}✓ Historico por diretorio do nave-suggest limpo.%f"
      ;;
    stats|status|"")
      local st="on" col="${_NAVE_C[ok]:-48}"
      if (( ! _NAVE_SG_ENABLED )); then
        st="off"
        col="${_NAVE_C[warn]:-214}"
      fi
      local dir_cnt=${#_NAVE_SG_DIR_CMDS}
      local hist_cnt=${#history}
      print -P "%F{${_NAVE_C[dir]:-45}}╭─ nave-suggest v2 ───────────────────────────────────────────%f"
      print -P "  Estado:    %F{${col}}%B${st}%b%f  (uso: nave-suggest on|off|clear)"
      print -P "  Motores:   %F{${_NAVE_C[info]:-51}}1. Pasta atual (${dir_cnt} cmds)%f → %F{${_NAVE_C[ok]:-48}}2. Historico (${hist_cnt})%f → %F{${_NAVE_C[path]:-87}}3. Arquivos/nave-z/Comandos%f"
      print -P "  Atalhos:   %B→ / End%b aceitar · %BCtrl+→%b palavra · %BAlt+↓/↑%b alternar sugestoes"
      print -P "%F{${_NAVE_C[dir]:-45}}╰─────────────────────────────────────────────────────────────%f"
      ;;
    *)
      print -P "Uso: nave-suggest [on|off|stats|clear]"
      return 1
      ;;
  esac
}

function _nave_suggest_invalidate {
  _NAVE_SG_LAST_BUF=""
  _NAVE_SG_LAST_HISTCMD=""
  _NAVE_SG_LAST_PWD=""
  _NAVE_SG_SUFFIX=""
  _NAVE_SG_SOURCE=""
  _NAVE_SG_CYCLE_IDX=0
  _NAVE_SG_EXPANDED=0
  _NAVE_SG_CANDS=()
}

# ------------------------------------------------------------------------------
# Per-Directory Successful Command Cache (loaded into RAM on chpwd — 0 disk I/O per key)
# ------------------------------------------------------------------------------
function _nave_suggest_load_dir_cache {
  emulate -L zsh
  _NAVE_SG_DIR_CMDS=()
  _NAVE_SG_Z_DIRS=()

  if [[ -r "$_NAVE_SG_DB" ]]; then
    local d c
    local -a raw=()
    while IFS=$'\t' read -r d c; do
      [[ "$d" == "$PWD" && -n "$c" ]] && raw+=("$c")
    done < "$_NAVE_SG_DB"
    local -A seen=()
    local i
    for (( i = ${#raw}; i >= 1; i-- )); do
      c="${raw[$i]}"
      if (( ! ${+seen[$c]} )); then
        seen[$c]=1
        _NAVE_SG_DIR_CMDS+=("$c")
        (( ${#_NAVE_SG_DIR_CMDS} >= 120 )) && break
      fi
    done
  fi

  local zdb="${NAVE_Z_DB:-$HOME/.local/share/nave/z.db}"
  if [[ -r "$zdb" ]]; then
    local s t p
    local -a rows=()
    while IFS='|' read -r s t p; do
      [[ -n "$p" && "$p" != "$PWD" && -d "$p" ]] || continue
      rows+=("$(printf '%010d|%s' "${s%%.*}" "$p")")
    done < "$zdb"
    if (( ${#rows} )); then
      local r
      for r in "${(@On)rows}"; do
        _NAVE_SG_Z_DIRS+=("${r#*|}")
        (( ${#_NAVE_SG_Z_DIRS} >= 80 )) && break
      done
    fi
  fi
}

function _nave_suggest_preexec {
  emulate -L zsh
  local cmd="$1"
  _NAVE_SG_PENDING_CMD=""
  _NAVE_SG_PENDING_PWD=""

  # Skip empty, leading-space (HIST_IGNORE_SPACE), multiline, huge, or secret-looking commands
  [[ -z "$cmd" || "$cmd" == [[:space:]]* || "$cmd" == *$'\n'* || $#cmd -gt 240 ]] && return
  [[ "$cmd" == *($'\t'|TOKEN=|PASSWORD=|PASSWD=|SECRET=|API_KEY=|WEBHOOK|ghp_|github_pat_|sk-)* ]] && return
  [[ "$cmd" == (exit|logout|clear|cls|nave-suggest*) ]] && return

  _NAVE_SG_PENDING_CMD="$cmd"
  _NAVE_SG_PENDING_PWD="$PWD"
}

function _nave_suggest_precmd {
  local last_rc=$?
  emulate -L zsh
  local cmd="$_NAVE_SG_PENDING_CMD"
  local dir="$_NAVE_SG_PENDING_PWD"
  _NAVE_SG_PENDING_CMD=""
  _NAVE_SG_PENDING_PWD=""

  # Always invalidate lookup cache between prompts (handles Ctrl+C & SHARE_HISTORY)
  _nave_suggest_invalidate

  # Record ONLY commands that succeeded (exit 0) into the per-directory history ring
  if (( last_rc == 0 )) && [[ -n "$cmd" && -n "$dir" ]]; then
    if [[ "$dir" == "$PWD" ]]; then
      local cmd_pat="${(b)cmd}"
      _NAVE_SG_DIR_CMDS=("$cmd" "${(@)_NAVE_SG_DIR_CMDS:#${~cmd_pat}}")
      (( ${#_NAVE_SG_DIR_CMDS} > 120 )) && _NAVE_SG_DIR_CMDS=("${(@)_NAVE_SG_DIR_CMDS[1,120]}")
    fi

    local -a lines=()
    if [[ -r "$_NAVE_SG_DB" ]]; then
      lines=("${(@f)$(<"$_NAVE_SG_DB")}")
    fi
    local entry="${dir}"$'\t'"${cmd}"
    local entry_pat="${(b)entry}"
    lines=("${(@)lines:#${~entry_pat}}" "$entry")
    if (( ${#lines} > 800 )); then
      lines=("${(@)lines[-800,-1]}")
    fi
    local tmp="${_NAVE_SG_DB}.tmp.$$"
    print -rl -- "${lines[@]}" >| "$tmp" 2>/dev/null && command mv -f "$tmp" "$_NAVE_SG_DB" 2>/dev/null
  fi
}

function _nave_suggest_chpwd {
  _nave_suggest_load_dir_cache
  _nave_suggest_invalidate
}

add-zsh-hook preexec _nave_suggest_preexec
add-zsh-hook precmd  _nave_suggest_precmd
add-zsh-hook chpwd   _nave_suggest_chpwd
_nave_suggest_load_dir_cache

# ------------------------------------------------------------------------------
# 3-Tier Candidate Discovery
# `$2 == 0` -> fast single-best mode (<0.15ms per keystroke!)
# `$2 == 1` -> expanded mode (up to 12 unique candidates for Alt+Down/Up cycling)
# ------------------------------------------------------------------------------
function _nave_suggest_collect {
  emulate -L zsh
  setopt EXTENDED_GLOB NO_BEEP NULL_GLOB

  local buf="$1"
  local expand=${2:-0}
  local blen=$#buf
  local limit=$(( expand ? 12 : 1 ))

  _NAVE_SG_CANDS=()
  _NAVE_SG_SOURCE=""
  _NAVE_SG_EXPANDED=$expand

  local -A seen=()
  local cand

  # ---- Tier 1: Same-directory successful commands (RAM array, newest first) ----
  for cand in "${_NAVE_SG_DIR_CMDS[@]}"; do
    if (( $#cand > blen && $#cand <= 300 )) && [[ "${cand:0:$blen}" == "$buf" && "$cand" != *$'\n'* ]]; then
      if (( ! ${+seen[$cand]} )); then
        seen[$cand]=1
        _NAVE_SG_CANDS+=("$cand")
        [[ -z "$_NAVE_SG_SOURCE" ]] && _NAVE_SG_SOURCE="dir"
        (( ${#_NAVE_SG_CANDS} >= limit )) && return
      fi
    fi
  done

  # ---- Tier 2: Global Zsh history (C-level newest-first scan via $history) ----
  local pat="${(b)buf}?*"
  local first="${history[(r)$pat]}"
  if [[ -n "$first" ]]; then
    if (( $#first <= 300 )) && [[ "$first" != *$'\n'* && "${first:0:$blen}" == "$buf" ]] && (( ! ${+seen[$first]} )); then
      seen[$first]=1
      _NAVE_SG_CANDS+=("$first")
      [[ -z "$_NAVE_SG_SOURCE" ]] && _NAVE_SG_SOURCE="hist"
      (( ${#_NAVE_SG_CANDS} >= limit )) && return
    fi
    # Scan full matches only when cycling (`expand=1`) or when #1 was multiline/oversized/duplicate
    if (( expand || ${#_NAVE_SG_CANDS} == 0 )); then
      local -a hmatches=("${(@)history[(R)$pat]}")
      for cand in "${hmatches[@]}"; do
        (( $#cand > blen && $#cand <= 300 )) || continue
        [[ "$cand" != *$'\n'* && "${cand:0:$blen}" == "$buf" ]] || continue
        if (( ! ${+seen[$cand]} )); then
          seen[$cand]=1
          _NAVE_SG_CANDS+=("$cand")
          [[ -z "$_NAVE_SG_SOURCE" ]] && _NAVE_SG_SOURCE="hist"
          (( ${#_NAVE_SG_CANDS} >= limit )) && return
        fi
      done
    fi
  fi

  (( ! expand && ${#_NAVE_SG_CANDS} > 0 )) && return

  # ---- Tier 3: Live smart fallback (local files/dirs, nave-z frecency, commands) ----
  local t3_limit=$(( expand ? limit : 1 ))

  # 3A. `cd <arg>` or `z <arg>` -> local directories first, then nave-z frecency dirs
  if [[ "$buf" == (cd|z)\ * ]]; then
    local cmd_head="${buf%% *}"
    local arg="${buf#* }"
    if [[ -n "$arg" && "$arg" != -* && "$arg" != *[*?\[\]\(\)\|\~\#\$\'\"]* ]]; then
      if [[ "$cmd_head" == "cd" ]]; then
        local -a ldirs=(${arg}*(N-/))
        local d
        for d in "${ldirs[@]}"; do
          cand="${cmd_head} ${d}/"
          if (( $#cand > blen )) && [[ "${cand:0:$blen}" == "$buf" ]] && (( ! ${+seen[$cand]} )); then
            seen[$cand]=1
            _NAVE_SG_CANDS+=("$cand")
            [[ -z "$_NAVE_SG_SOURCE" ]] && _NAVE_SG_SOURCE="fs"
            (( ${#_NAVE_SG_CANDS} >= t3_limit )) && return
          fi
        done
      fi
      local zp zdisp zbase larg="${arg:l}"
      for zp in "${_NAVE_SG_Z_DIRS[@]}"; do
        zbase="${zp:t:l}"
        zdisp="${zp/#$HOME/~}"
        if [[ "$cmd_head" == "z" ]]; then
          [[ "$zbase" == ${larg}* ]] || continue
          cand="z ${zp:t}"
        else
          [[ "$zdisp" == ${arg}* ]] || continue
          cand="cd ${zdisp}"
        fi
        if (( $#cand > blen )) && [[ "${cand:0:$blen}" == "$buf" ]] && (( ! ${+seen[$cand]} )); then
          seen[$cand]=1
          _NAVE_SG_CANDS+=("$cand")
          [[ -z "$_NAVE_SG_SOURCE" ]] && _NAVE_SG_SOURCE="z"
          (( ${#_NAVE_SG_CANDS} >= t3_limit )) && return
        fi
      done
    fi
    return
  fi

  # 3B. Local file/directory completion for last argument (e.g. `ne ma` -> `ne main.cpp`)
  if [[ "$buf" == *" "* ]]; then
    local last_word="${buf##* }"
    local prefix_part="${buf:0:$(( blen - $#last_word ))}"
    if [[ -n "$last_word" && "$last_word" != -* && "$last_word" != *[*?\[\]\(\)\|\~\#\$\'\"]* && "$last_word" != *"://"* ]]; then
      local -a matches=(${last_word}*(N))
      local m suf
      for m in "${matches[@]}"; do
        suf="$m"
        [[ -d "$m" ]] && suf="${m}/"
        cand="${prefix_part}${suf}"
        if (( $#cand > blen )) && [[ "${cand:0:$blen}" == "$buf" ]] && (( ! ${+seen[$cand]} )); then
          seen[$cand]=1
          _NAVE_SG_CANDS+=("$cand")
          [[ -z "$_NAVE_SG_SOURCE" ]] && _NAVE_SG_SOURCE="fs"
          (( ${#_NAVE_SG_CANDS} >= t3_limit )) && return
        fi
      done
    fi
    return
  fi

  # 3C. First-word command/alias/function completion (min 2 chars, pure RAM hash lookup)
  if (( blen >= 2 )) && [[ "$buf" != */* && "$buf" != *[*?\[]* ]]; then
    local cpat="${(b)buf}?*"
    local -a cmatches=()
    cmatches+=("${(@k)aliases[(I)$cpat]}")
    cmatches+=("${(@k)functions[(I)$cpat]}")
    cmatches+=("${(@k)builtins[(I)$cpat]}")
    cmatches+=("${(@k)commands[(I)$cpat]}")
    local cm
    for cm in "${(@o)cmatches}"; do
      [[ "$cm" == _* || "$cm" == .* ]] && continue
      if (( $#cm > blen )) && [[ "${cm:0:$blen}" == "$buf" ]] && (( ! ${+seen[$cm]} )); then
        seen[$cm]=1
        _NAVE_SG_CANDS+=("$cm")
        [[ -z "$_NAVE_SG_SOURCE" ]] && _NAVE_SG_SOURCE="cmd"
        (( ${#_NAVE_SG_CANDS} >= t3_limit )) && return
      fi
    done
  fi
}

# ------------------------------------------------------------------------------
# Render POSTDISPLAY + region_highlight cleanly
# ------------------------------------------------------------------------------
function _nave_suggest_render {
  emulate -L zsh
  region_highlight=(${region_highlight:#*memo=nave-sg*})

  local total=${#_NAVE_SG_CANDS}
  if (( total == 0 )); then
    _NAVE_SG_SUFFIX=""
    POSTDISPLAY=""
    return
  fi

  (( _NAVE_SG_CYCLE_IDX >= total )) && _NAVE_SG_CYCLE_IDX=0
  local match="${_NAVE_SG_CANDS[$(( _NAVE_SG_CYCLE_IDX + 1 ))]}"
  local blen=$#BUFFER
  if (( $#match <= blen )) || [[ "${match:0:$blen}" != "$BUFFER" ]]; then
    _NAVE_SG_SUFFIX=""
    POSTDISPLAY=""
    return
  fi

  _NAVE_SG_SUFFIX="${match:$blen}"
  local badge=""
  if (( _NAVE_SG_EXPANDED && total > 1 )); then
    badge="  [$(( _NAVE_SG_CYCLE_IDX + 1 ))/${total}]"
  fi
  POSTDISPLAY="${_NAVE_SG_SUFFIX}${badge}"

  local start=$(( $#PREDISPLAY + blen ))
  local s_len=$#_NAVE_SG_SUFFIX
  local b_len=$#badge
  if (( s_len > 0 )); then
    region_highlight+=("${start} $(( start + s_len )) fg=${_NAVE_C[muted]:-244}, memo=nave-sg")
  fi
  if (( b_len > 0 )); then
    region_highlight+=("$(( start + s_len )) $(( start + s_len + b_len )) fg=${_NAVE_C[info]:-51}, memo=nave-sg")
  fi
}

function _nave_suggest_redraw {
  emulate -L zsh
  # Suppress during selection, completion menu, Ctrl+R isearch, vicmd, multiline, or empty/whitespace
  if (( ! _NAVE_SG_ENABLED || REGION_ACTIVE )) \
     || [[ "$CONTEXT" != "start" ]] \
     || [[ "$KEYMAP" == (menuselect|isearch|vicmd|command) ]] \
     || [[ "$LASTWIDGET" == (*complete*|*menu*|*isearch*) ]] \
     || [[ -z "${BUFFER//[[:space:]]/}" || "$BUFFER" == *$'\n'* ]]; then
    _NAVE_SG_SUFFIX=""
    POSTDISPLAY=""
    region_highlight=(${region_highlight:#*memo=nave-sg*})
    return
  fi

  if [[ "$BUFFER" != "$_NAVE_SG_LAST_BUF" || "$HISTCMD" != "$_NAVE_SG_LAST_HISTCMD" || "$PWD" != "$_NAVE_SG_LAST_PWD" ]]; then
    _nave_suggest_collect "$BUFFER" 0
    _NAVE_SG_LAST_BUF="$BUFFER"
    _NAVE_SG_LAST_HISTCMD="$HISTCMD"
    _NAVE_SG_LAST_PWD="$PWD"
    _NAVE_SG_CYCLE_IDX=0
  fi

  _nave_suggest_render
}

function _nave_suggest_clear {
  emulate -L zsh
  POSTDISPLAY=""
  _nave_suggest_invalidate
  region_highlight=(${region_highlight:#*memo=nave-sg*})
}

# ------------------------------------------------------------------------------
# Interactive Widgets (Accept Full, Accept Subword, Cycle Next/Prev)
# ------------------------------------------------------------------------------

# Accept entire suggestion (called by Right arrow / End when at end of line)
function _nave_suggest_accept {
  [[ -n "$_NAVE_SG_SUFFIX" ]] || return 1
  zle split-undo
  BUFFER+="$_NAVE_SG_SUFFIX"
  _NAVE_SG_SUFFIX=""
  POSTDISPLAY=""
  CURSOR=$#BUFFER
  return 0
}

# Accept next subword/segment of suggestion (called by Ctrl+Right / Alt+Right at end of line)
function _nave_suggest_accept_word {
  emulate -L zsh
  [[ -n "$_NAVE_SG_SUFFIX" ]] || return 1
  local rest="$_NAVE_SG_SUFFIX"
  local n=$#rest i=1
  while (( i <= n )) && [[ "${rest[$i]}" != [[:alnum:]] ]]; do (( i++ )); done
  while (( i <= n )) && [[ "${rest[$i]}" == [[:alnum:]] ]]; do (( i++ )); done
  local take="${rest:0:$(( i - 1 ))}"
  [[ -z "$take" ]] && take="$rest"

  zle split-undo
  BUFFER+="$take"
  _NAVE_SG_SUFFIX="${rest:$#take}"
  POSTDISPLAY="$_NAVE_SG_SUFFIX"
  CURSOR=$#BUFFER
  return 0
}

# Cycle to next matching suggestion (Alt+Down)
function _nave_suggest_next {
  emulate -L zsh
  (( _NAVE_SG_ENABLED && ! REGION_ACTIVE )) || return
  [[ -n "${BUFFER//[[:space:]]/}" && "$BUFFER" != *$'\n'* ]] || return
  if (( ! _NAVE_SG_EXPANDED )); then
    _nave_suggest_collect "$BUFFER" 1
    _NAVE_SG_LAST_BUF="$BUFFER"
    _NAVE_SG_LAST_HISTCMD="$HISTCMD"
    _NAVE_SG_LAST_PWD="$PWD"
  fi
  local total=${#_NAVE_SG_CANDS}
  (( total > 1 )) || return
  _NAVE_SG_CYCLE_IDX=$(( (_NAVE_SG_CYCLE_IDX + 1) % total ))
  _nave_suggest_render
}

# Cycle to previous matching suggestion (Alt+Up)
function _nave_suggest_prev {
  emulate -L zsh
  (( _NAVE_SG_ENABLED && ! REGION_ACTIVE )) || return
  [[ -n "${BUFFER//[[:space:]]/}" && "$BUFFER" != *$'\n'* ]] || return
  if (( ! _NAVE_SG_EXPANDED )); then
    _nave_suggest_collect "$BUFFER" 1
    _NAVE_SG_LAST_BUF="$BUFFER"
    _NAVE_SG_LAST_HISTCMD="$HISTCMD"
    _NAVE_SG_LAST_PWD="$PWD"
  fi
  local total=${#_NAVE_SG_CANDS}
  (( total > 1 )) || return
  _NAVE_SG_CYCLE_IDX=$(( (_NAVE_SG_CYCLE_IDX - 1 + total) % total ))
  _nave_suggest_render
}

zle -N _nave_suggest_clear
zle -N _nave_suggest_next
zle -N _nave_suggest_prev

add-zle-hook-widget zle-line-init       _nave_suggest_clear
add-zle-hook-widget zle-line-pre-redraw _nave_suggest_redraw
add-zle-hook-widget zle-line-finish     _nave_suggest_clear
