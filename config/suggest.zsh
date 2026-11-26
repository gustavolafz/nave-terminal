# ==============================================================================
# ~/.config/nave/suggest.zsh
# nave-suggest — Pure Zsh 5.9 history autosuggestions (zero external plugins)
# Zero widget monkey-patching, <1ms lookup, full & partial-word accept.
# ==============================================================================

autoload -Uz add-zle-hook-widget
zmodload zsh/parameter 2>/dev/null

typeset -g _NAVE_SG_ENABLED=1
[[ -r "${NAVE_CONFIG_DIR:-$HOME/.config/nave}/suggest" ]] && _NAVE_SG_ENABLED="$(<"${NAVE_CONFIG_DIR:-$HOME/.config/nave}/suggest")"
typeset -g _NAVE_SG_LAST_BUF=""
typeset -g _NAVE_SG_LAST_MATCH=""

function nave-suggest {
  local cfg="${NAVE_CONFIG_DIR:-$HOME/.config/nave}/suggest"
  case "$1" in
    on|1)
      _NAVE_SG_ENABLED=1
      _NAVE_SG_LAST_BUF=""
      print -r -- "1" > "$cfg"
      print -P "%F{${_NAVE_C[ok]:-48}}nave-suggest ativado.%f"
      ;;
    off|0)
      _NAVE_SG_ENABLED=0
      _NAVE_SG_LAST_BUF=""
      print -r -- "0" > "$cfg"
      print -P "%F{${_NAVE_C[warn]:-214}}nave-suggest desativado.%f"
      ;;
    *)
      local st="on"
      (( _NAVE_SG_ENABLED )) || st="off"
      print -P "nave-suggest: %B${st}%b (uso: nave-suggest on|off)"
      ;;
  esac
}

function _nave_suggest_find {
  local buf="$1"
  local pat="${(b)buf}?*"

  # Fast C-level hash check: exit in ~0.1ms if no matching history entry exists at all
  local any="${history[(r)$pat]}"
  if [[ -z "$any" ]]; then
    REPLY=""
    return
  fi

  # Guaranteed match exists: scan strictly newest-to-oldest from $HISTCMD for recency
  local i=$(( HISTCMD - 1 ))
  local min=$(( HISTCMD > 5000 ? HISTCMD - 5000 : 1 ))
  local cand
  while (( i >= min )); do
    cand="${history[$i]}"
    if [[ -n "$cand" && "$cand" == ${~pat} ]]; then
      REPLY="$cand"
      return
    fi
    (( i-- ))
  done

  REPLY="$any"
}

function _nave_suggest_redraw {
  region_highlight=(${region_highlight:#*, memo=nave-sg})

  if (( ! _NAVE_SG_ENABLED || REGION_ACTIVE )) || [[ -z "$BUFFER" || "$BUFFER" == *$'\n'* ]]; then
    POSTDISPLAY=""
    return
  fi

  local match
  if [[ "$BUFFER" == "$_NAVE_SG_LAST_BUF" ]]; then
    match="$_NAVE_SG_LAST_MATCH"
  else
    _nave_suggest_find "$BUFFER"
    match="$REPLY"
    _NAVE_SG_LAST_BUF="$BUFFER"
    _NAVE_SG_LAST_MATCH="$match"
  fi

  if [[ -n "$match" ]]; then
    POSTDISPLAY="${match#$BUFFER}"
    local b_len=$#BUFFER
    local p_len=$#POSTDISPLAY
    if (( p_len > 0 )); then
      region_highlight+=("${b_len} $(( b_len + p_len )) fg=${_NAVE_C[muted]:-244}, memo=nave-sg")
    fi
  else
    POSTDISPLAY=""
  fi
}

function _nave_suggest_clear {
  POSTDISPLAY=""
  _NAVE_SG_LAST_BUF=""
  _NAVE_SG_LAST_MATCH=""
  region_highlight=(${region_highlight:#*, memo=nave-sg})
}

# Accept entire suggestion (called by Right arrow / End when at end of line)
function _nave_suggest_accept {
  [[ -n "$POSTDISPLAY" ]] || return 1
  BUFFER+="$POSTDISPLAY"
  POSTDISPLAY=""
  CURSOR=$#BUFFER
  return 0
}

# Accept next subword/segment of suggestion (called by Ctrl+Right / Alt+Right at end of line)
function _nave_suggest_accept_word {
  [[ -n "$POSTDISPLAY" ]] || return 1
  local rest="$POSTDISPLAY"
  local take=""
  # Consume leading whitespace/punctuation separators plus the following word
  if [[ "$rest" =~ '^([^[:alnum:]]*[[:alnum:]]+|[^[:alnum:]]+)' ]]; then
    take="$MATCH"
  else
    take="$rest"
  fi
  BUFFER+="$take"
  POSTDISPLAY="${rest#$take}"
  CURSOR=$#BUFFER
  return 0
}

add-zle-hook-widget zle-line-pre-redraw _nave_suggest_redraw
add-zle-hook-widget zle-line-finish _nave_suggest_clear
