# ==============================================================================
# ~/.config/nave/init.zsh
# Nave Terminal v4 — 100% Standalone Zsh Framework (Zero Oh My Zsh / Zero external plugins)
# Usage in ~/.zshrc:
#   [[ -r "$HOME/.config/nave/init.zsh" ]] && source "$HOME/.config/nave/init.zsh"
# ==============================================================================

# Guard against double-loading in non-interactive subshells
[[ -o interactive ]] || return 0

# Core Zsh C modules (datetime for $EPOCHSECONDS, parameter for $history/$commands, complist for menu select)
zmodload zsh/datetime 2>/dev/null
zmodload zsh/parameter 2>/dev/null
zmodload zsh/complist 2>/dev/null

typeset -g NAVE_CONFIG_DIR="${NAVE_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/nave}"
typeset -g NAVE_CACHE_DIR="${NAVE_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/nave}"
typeset -g NAVE_DATA_DIR="${NAVE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/nave}"
[[ -d "$NAVE_CONFIG_DIR" ]] || mkdir -p "$NAVE_CONFIG_DIR"
[[ -d "$NAVE_CACHE_DIR"  ]] || mkdir -p "$NAVE_CACHE_DIR"
[[ -d "$NAVE_DATA_DIR"   ]] || mkdir -p "$NAVE_DATA_DIR"

# ------------------------------------------------------------------------------
# Shell Options & History (replaces Oh My Zsh lib/*.zsh)
# ------------------------------------------------------------------------------
setopt PROMPT_SUBST
setopt EXTENDED_GLOB
setopt INTERACTIVE_COMMENTS
setopt MULTIOS
setopt LONG_LIST_JOBS
setopt AUTO_CD
setopt AUTO_PUSHD
setopt PUSHD_IGNORE_DUPS
setopt PUSHD_SILENT
setopt AUTO_MENU
setopt COMPLETE_IN_WORD
setopt ALWAYS_TO_END
setopt NO_FLOW_CONTROL
setopt IGNORE_EOF

HISTFILE="${HISTFILE:-$HOME/.zsh_history}"
HISTSIZE="${HISTSIZE:-50000}"
SAVEHIST="${SAVEHIST:-50000}"
setopt APPEND_HISTORY
setopt EXTENDED_HISTORY
setopt HIST_EXPIRE_DUPS_FIRST
setopt HIST_IGNORE_DUPS
setopt HIST_IGNORE_ALL_DUPS
setopt HIST_IGNORE_SPACE
setopt HIST_REDUCE_BLANKS
setopt HIST_VERIFY
setopt SHARE_HISTORY

# Emacs base keymap (VS Code keybindings layer on top in keybindings.zsh)
bindkey -e

# ------------------------------------------------------------------------------
# Fast Cached Completion Engine (regenerates dump once every 24h)
# ------------------------------------------------------------------------------
autoload -Uz compinit
() {
  local zcd="${NAVE_CACHE_DIR}/zcompdump-${ZSH_VERSION}"
  if [[ -f "$zcd" && -n "$zcd"(#qN.mh-24) ]]; then
    compinit -C -d "$zcd"
  else
    compinit -i -d "$zcd"
    { zcompile "$zcd" 2>/dev/null } &!
  fi
}

# Interactive colored menu completion + case-insensitive substring matching
zstyle ':completion:*' menu select
zstyle ':completion:*' matcher-list 'm:{a-zA-Z}={A-Za-z}' 'r:|=*' 'l:|=* r:|=*'
zstyle ':completion:*' special-dirs true
zstyle ':completion:*' group-name ''
zstyle ':completion:*:descriptions' format '%F{244}── %d ──%f'

# Clean LS_COLORS (fixes unreadable bright-green blocks on 0777 WSL folders)
if command -v dircolors >/dev/null 2>&1; then
  eval "$(dircolors -b)"
fi
LS_COLORS="${LS_COLORS:+$(printf '%s' "$LS_COLORS" | sed -E 's/(^|:)(ow|tw|st)=[^:]*//g; s/^://'):}ow=01;34:tw=01;34:st=01;36"
export LS_COLORS
zstyle ':completion:*' list-colors "${(s.:.)LS_COLORS}" 'ma=48;5;238;38;5;81;1'

# Shift+Tab moves backwards inside menu completion
bindkey -M menuselect "^[[Z" reverse-menu-complete 2>/dev/null

# ------------------------------------------------------------------------------
# Load Native Nave Modules (in exact dependency order)
# ------------------------------------------------------------------------------
typeset -g _NAVE_INIT_ACTIVE=1

for _nave_mod in theme.zsh z.zsh toolkit.zsh suggest.zsh keybindings.zsh highlight.zsh; do
  [[ -r "${NAVE_CONFIG_DIR}/${_nave_mod}" ]] && source "${NAVE_CONFIG_DIR}/${_nave_mod}"
done
unset _nave_mod _NAVE_INIT_ACTIVE

# Welcome status bar
(( ${+functions[_nave_show_welcome]} )) && _nave_show_welcome
