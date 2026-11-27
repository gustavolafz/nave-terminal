# ==============================================================================
# ~/.config/nave/keybindings.zsh
# VS Code-style text editing in Zsh (selection, subword nav, undo/redo, clipboard)
# Integrated natively with nave-suggest (Right/End accept, Ctrl+Right word accept)
# ==============================================================================

# Prevent Ctrl+S / Ctrl+Q from freezing the terminal
setopt NO_FLOW_CONTROL
[[ -t 0 ]] && stty -ixon 2>/dev/null

# Prevent Ctrl+D from closing the WSL tab/shell (Ctrl+D selects word like VS Code!)
setopt IGNORE_EOF

# Allow `# comments` in interactive commands (makes Ctrl+/ + Enter save to history!)
setopt INTERACTIVE_COMMENTS

# Word boundaries like VS Code: Ctrl+Left/Right/Backspace/Delete stop at /, -, _, ., =, :
WORDCHARS=''

# High-contrast Obsidian selection highlight
zle_highlight=(region:bg=238,fg=51,bold)

# Ensure edit-command-line and prefix history search widgets are loaded
autoload -Uz edit-command-line up-line-or-beginning-search down-line-or-beginning-search
zle -N edit-command-line
zle -N up-line-or-beginning-search
zle -N down-line-or-beginning-search

# ------------------------------------------------------------------------------
# Clipboard helper (WSL UTF-8 chcp 65001 + clip.exe -> wl-copy -> xclip -> pbcopy)
function _nave_copy_text {
  local text="$1"
  [[ -z "$text" ]] && return
  if command -v cmd.exe >/dev/null 2>&1 && [[ -d /mnt/c ]]; then
    (cd /mnt/c && printf '%s' "$text" | cmd.exe /d /c "chcp 65001 >nul && clip.exe" >/dev/null 2>&1)
  elif command -v clip.exe >/dev/null 2>&1; then
    printf '%s' "$text" | clip.exe
  elif command -v wl-copy >/dev/null 2>&1; then
    printf '%s' "$text" | wl-copy
  elif command -v xclip >/dev/null 2>&1; then
    printf '%s' "$text" | xclip -selection clipboard
  elif command -v pbcopy >/dev/null 2>&1; then
    printf '%s' "$text" | pbcopy
  fi
}

function _nave_get_region {
  local start=$(( MARK < CURSOR ? MARK : CURSOR ))
  local end=$(( MARK > CURSOR ? MARK : CURSOR ))
  printf '%s' "${BUFFER:$start:$((end - start))}"
}

# ------------------------------------------------------------------------------
# Ctrl+C smart behavior (CopyOrCancelLine — matches terminal + VS Code selection):
#   - With keyboard selection active (Shift+Arrows / Ctrl+A / Ctrl+L / Ctrl+D): copies selection
#   - Without selection (or while a command runs): cancels / sends SIGINT normally
# ------------------------------------------------------------------------------
function _nave_copy_if_selected {
  if (( REGION_ACTIVE )); then
    _nave_copy_text "$(_nave_get_region)"
    REGION_ACTIVE=0
    region_highlight=(${region_highlight:#*memo=nave-sel*})
    _NAVE_INT_COPIED=1
  else
    _NAVE_INT_COPIED=0
  fi
}

function TRAPINT {
  _NAVE_INT_COPIED=0
  if zle; then
    zle _nave_copy_if_selected
    if (( _NAVE_INT_COPIED )); then
      zle -R
      return 0
    fi
    (( ${+widgets[_nave_suggest_clear]} )) && zle _nave_suggest_clear 2>/dev/null
  fi
  return $(( 128 + $1 ))
}

# ------------------------------------------------------------------------------
# Selection-aware movement widgets (Shift+Arrows, Ctrl+Shift+Arrows, Shift+Home/End)
# ------------------------------------------------------------------------------
function _nave_select_motion {
  local motion="$1"
  if (( ! REGION_ACTIVE )); then
    MARK=$CURSOR
    REGION_ACTIVE=1
  fi
  zle "$motion"
  (( CURSOR == MARK )) && REGION_ACTIVE=0
}

function _nave_deselect_motion {
  local motion="$1"
  local dir="$2" # left | right | other
  if (( REGION_ACTIVE )); then
    local start=$(( MARK < CURSOR ? MARK : CURSOR ))
    local end=$(( MARK > CURSOR ? MARK : CURSOR ))
    REGION_ACTIVE=0
    if [[ "$dir" == "left" ]]; then
      CURSOR=$start
      return
    elif [[ "$dir" == "right" ]]; then
      CURSOR=$end
      return
    fi
  fi
  zle "$motion"
}

function _nave_sel_left       { _nave_select_motion backward-char; }
function _nave_sel_right      { _nave_select_motion forward-char; }
function _nave_sel_up         { _nave_select_motion up-line; }
function _nave_sel_down       { _nave_select_motion down-line; }
function _nave_sel_word_left  { _nave_select_motion backward-word; }
function _nave_sel_word_right { _nave_select_motion forward-word; }
function _nave_sel_home       { _nave_select_motion beginning-of-line; }
function _nave_sel_end        { _nave_select_motion end-of-line; }

function _nave_move_left { _nave_deselect_motion backward-char left; }

function _nave_move_right {
  if (( ! REGION_ACTIVE && CURSOR == $#BUFFER )) && (( ${+functions[_nave_suggest_accept]} )); then
    _nave_suggest_accept && return
  fi
  _nave_deselect_motion forward-char right
}

function _nave_move_word_left { _nave_deselect_motion backward-word other; }

function _nave_move_word_right {
  if (( ! REGION_ACTIVE && CURSOR == $#BUFFER )) && (( ${+functions[_nave_suggest_accept_word]} )); then
    _nave_suggest_accept_word && return
  fi
  _nave_deselect_motion forward-word other
}

function _nave_move_home { _nave_deselect_motion beginning-of-line other; }

function _nave_move_end {
  if (( ! REGION_ACTIVE && CURSOR == $#BUFFER )) && (( ${+functions[_nave_suggest_accept]} )); then
    _nave_suggest_accept && return
  fi
  _nave_deselect_motion end-of-line other
}

function _nave_hist_up {
  REGION_ACTIVE=0
  zle up-line-or-beginning-search
}

function _nave_hist_down {
  REGION_ACTIVE=0
  zle down-line-or-beginning-search
}

# Ctrl+A / Ctrl+L: Select entire command line (like VS Code!)
function _nave_select_all {
  [[ -z "$BUFFER" ]] && return
  MARK=0
  CURSOR=$#BUFFER
  REGION_ACTIVE=1
}

# Ctrl+D: Select word under/before cursor (like VS Code!).
# Pressing Ctrl+D a second time expands selection to the full path/argument.
function _nave_select_word {
  [[ -z "$BUFFER" ]] && return
  local len=$#BUFFER
  if (( REGION_ACTIVE )); then
    local s=$(( MARK < CURSOR ? MARK : CURSOR ))
    local e=$(( MARK > CURSOR ? MARK : CURSOR ))
    while (( s > 0 )) && [[ "${BUFFER:$((s-1)):1}" != [[:space:]] ]]; do (( s-- )); done
    while (( e < len )) && [[ "${BUFFER:$e:1}" != [[:space:]] ]]; do (( e++ )); done
    MARK=$s
    CURSOR=$e
    REGION_ACTIVE=1
    return
  fi
  local pos=$CURSOR
  if (( pos >= len )) || [[ "${BUFFER:$pos:1}" != [[:alnum:]] ]]; then
    if (( pos > 0 )) && [[ "${BUFFER:$((pos-1)):1}" == [[:alnum:]] ]]; then
      (( pos-- ))
    elif (( pos >= len && pos > 0 )); then
      (( pos-- ))
    fi
  fi
  local s=$pos e=$pos
  if [[ "${BUFFER:$pos:1}" == [[:alnum:]] ]]; then
    while (( s > 0 )) && [[ "${BUFFER:$((s-1)):1}" == [[:alnum:]] ]]; do (( s-- )); done
    while (( e < len )) && [[ "${BUFFER:$e:1}" == [[:alnum:]] ]]; do (( e++ )); done
  else
    while (( s > 0 )) && [[ "${BUFFER:$((s-1)):1}" != [[:space:]] ]]; do (( s-- )); done
    while (( e < len )) && [[ "${BUFFER:$e:1}" != [[:space:]] ]]; do (( e++ )); done
  fi
  (( e > s )) || return
  MARK=$s
  CURSOR=$e
  REGION_ACTIVE=1
}

# Shift+Enter / Alt+Enter: Insert newline without executing command
function _nave_insert_newline {
  (( REGION_ACTIVE )) && zle kill-region
  LBUFFER+=$'\n'
}

# Esc Esc: Toggle `sudo ` prefix on current (or previous) command
function _nave_sudo_toggle {
  REGION_ACTIVE=0
  [[ -z "$BUFFER" ]] && LBUFFER="$(fc -ln -1)"
  if [[ "$BUFFER" == sudo\ * ]]; then
    BUFFER="${BUFFER#sudo }"
    (( CURSOR = CURSOR > 5 ? CURSOR - 5 : 0 ))
  else
    BUFFER="sudo ${BUFFER}"
    (( CURSOR += 5 ))
  fi
}

# ------------------------------------------------------------------------------
# Selection-aware editing (Backspace, Delete, Ctrl+Backspace, Ctrl+Delete, Typing, Paste)
# ------------------------------------------------------------------------------
function _nave_backspace {
  if (( REGION_ACTIVE )); then
    zle kill-region
  else
    zle backward-delete-char
  fi
}

function _nave_delete {
  if (( REGION_ACTIVE )); then
    zle kill-region
  else
    zle delete-char
  fi
}

function _nave_kill_word_back {
  if (( REGION_ACTIVE )); then
    zle kill-region
  else
    zle backward-kill-word
  fi
}

function _nave_kill_word_fwd {
  if (( REGION_ACTIVE )); then
    zle kill-region
  else
    zle kill-word
  fi
}

# Ctrl+X: Cut selection (or entire line if nothing selected) to Windows clipboard
function _nave_cut {
  if (( REGION_ACTIVE )); then
    _nave_copy_text "$(_nave_get_region)"
    zle kill-region
  elif [[ -n "$BUFFER" ]]; then
    _nave_copy_text "$BUFFER"
    zle kill-whole-line
  fi
}

# Copy selection (or entire line) to Windows clipboard (Alt+W / Ctrl+Insert)
function _nave_copy {
  if (( REGION_ACTIVE )); then
    _nave_copy_text "$(_nave_get_region)"
    REGION_ACTIVE=0
    region_highlight=(${region_highlight:#*memo=nave-sel*})
  elif [[ -n "$BUFFER" ]]; then
    _nave_copy_text "$BUFFER"
  fi
}

# Ctrl+Shift+K: Delete entire line (VS Code shortcut)
function _nave_delete_line {
  REGION_ACTIVE=0
  zle kill-whole-line
}

# Shift+Alt+Down / Ctrl+Shift+D: Duplicate selection (or entire line)
function _nave_duplicate {
  if (( REGION_ACTIVE )); then
    local txt
    txt=$(_nave_get_region)
    local end=$(( MARK > CURSOR ? MARK : CURSOR ))
    REGION_ACTIVE=0
    CURSOR=$end
    LBUFFER+="$txt"
  elif [[ -n "$BUFFER" ]]; then
    BUFFER="${BUFFER} ${BUFFER}"
    CURSOR=$#BUFFER
  fi
}

# Ctrl+/ (or Ctrl+; on ABNT2): Toggle `# ` comment on current line
function _nave_toggle_comment {
  REGION_ACTIVE=0
  if [[ "$BUFFER" == \#\ * ]]; then
    BUFFER="${BUFFER#\# }"
    (( CURSOR = CURSOR > 2 ? CURSOR - 2 : 0 ))
  elif [[ "$BUFFER" == \#* ]]; then
    BUFFER="${BUFFER#\#}"
    (( CURSOR = CURSOR > 1 ? CURSOR - 1 : 0 ))
  else
    BUFFER="# ${BUFFER}"
    (( CURSOR += 2 ))
  fi
}

# Overwrite selection when typing or pasting (clean, zero plugin conflicts!)
function _nave_self_insert {
  (( REGION_ACTIVE )) && zle kill-region
  zle .self-insert
}

function _nave_bracketed_paste {
  (( REGION_ACTIVE )) && zle kill-region
  zle .bracketed-paste
}

# Register all widgets
for w in \
  _nave_sel_left _nave_sel_right _nave_sel_up _nave_sel_down _nave_sel_word_left _nave_sel_word_right _nave_sel_home _nave_sel_end \
  _nave_move_left _nave_move_right _nave_move_word_left _nave_move_word_right _nave_move_home _nave_move_end \
  _nave_hist_up _nave_hist_down _nave_select_all _nave_select_word _nave_insert_newline _nave_sudo_toggle \
  _nave_backspace _nave_delete _nave_kill_word_back _nave_kill_word_fwd \
  _nave_cut _nave_copy _nave_copy_if_selected _nave_delete_line _nave_duplicate _nave_toggle_comment
do
  zle -N "$w"
done
zle -N self-insert _nave_self_insert
zle -N bracketed-paste _nave_bracketed_paste

# ------------------------------------------------------------------------------
# Keybindings (Windows Terminal + VS Code integrated terminal + XTerm/CSI u codes)
# ------------------------------------------------------------------------------

# Basic movement (deselects region cleanly; Right/End/Ctrl+Right accept nave-suggest)
bindkey "^[[D"    _nave_move_left        # Left
bindkey "^[OD"    _nave_move_left        # Left (app mode)
bindkey "^[[C"    _nave_move_right       # Right
bindkey "^[OC"    _nave_move_right       # Right (app mode)
bindkey "^[[A"    _nave_hist_up          # Up (history prefix search)
bindkey "^[OA"    _nave_hist_up          # Up (app mode)
bindkey "^[[B"    _nave_hist_down        # Down (history prefix search)
bindkey "^[OB"    _nave_hist_down        # Down (app mode)
bindkey "^[[H"    _nave_move_home        # Home
bindkey "^[OH"    _nave_move_home        # Home (app mode)
bindkey "^[[1~"   _nave_move_home        # Home (rxvt/tmux)
bindkey "^[[1;5H" _nave_move_home        # Ctrl+Home
bindkey "^[[F"    _nave_move_end         # End
bindkey "^[OF"    _nave_move_end         # End (app mode)
bindkey "^[[4~"   _nave_move_end         # End (rxvt/tmux)
bindkey "^[[1;5F" _nave_move_end         # Ctrl+End
bindkey "^E"      _nave_move_end         # Ctrl+E (End of line)
bindkey "^[[1;5D" _nave_move_word_left   # Ctrl+Left
bindkey "^[[5D"   _nave_move_word_left   # Ctrl+Left (short CSI)
bindkey "^[O1;5D" _nave_move_word_left   # Ctrl+Left (app mode)
bindkey "^[[1;3D" _nave_move_word_left   # Alt+Left
bindkey "^[Od"    _nave_move_word_left   # Ctrl+Left (rxvt)
bindkey "^[b"     _nave_move_word_left   # Alt+B
bindkey "^[[1;5C" _nave_move_word_right  # Ctrl+Right
bindkey "^[[5C"   _nave_move_word_right  # Ctrl+Right (short CSI)
bindkey "^[O1;5C" _nave_move_word_right  # Ctrl+Right (app mode)
bindkey "^[[1;3C" _nave_move_word_right  # Alt+Right
bindkey "^[Oc"    _nave_move_word_right  # Ctrl+Right (rxvt)
bindkey "^[f"     _nave_move_word_right  # Alt+F
if (( ${+widgets[_nave_suggest_next]} )); then
  bindkey "^[[1;3B" _nave_suggest_next   # Alt+Down (cycle next suggestion)
  bindkey "^[[1;3A" _nave_suggest_prev   # Alt+Up   (cycle previous suggestion)
fi

# Selection (VS Code / Windows standard — XTerm, short CSI, and app-mode variants)
bindkey "^[[1;2D" _nave_sel_left         # Shift+Left
bindkey "^[[2D"   _nave_sel_left         # Shift+Left (short CSI)
bindkey "^[O1;2D" _nave_sel_left         # Shift+Left (app mode)
bindkey "^[[1;2C" _nave_sel_right        # Shift+Right
bindkey "^[[2C"   _nave_sel_right        # Shift+Right (short CSI)
bindkey "^[O1;2C" _nave_sel_right        # Shift+Right (app mode)
bindkey "^[[1;2A" _nave_sel_up           # Shift+Up (multiline)
bindkey "^[[1;2B" _nave_sel_down         # Shift+Down (multiline)
bindkey "^[[1;6D" _nave_sel_word_left    # Ctrl+Shift+Left
bindkey "^[[6D"   _nave_sel_word_left    # Ctrl+Shift+Left (short CSI)
bindkey "^[O1;6D" _nave_sel_word_left    # Ctrl+Shift+Left (app mode)
bindkey "^[O6D"   _nave_sel_word_left    # Ctrl+Shift+Left (app short)
bindkey "^[[1;4D" _nave_sel_word_left    # Alt+Shift+Left
bindkey "^[[1;6C" _nave_sel_word_right   # Ctrl+Shift+Right
bindkey "^[[6C"   _nave_sel_word_right   # Ctrl+Shift+Right (short CSI)
bindkey "^[O1;6C" _nave_sel_word_right   # Ctrl+Shift+Right (app mode)
bindkey "^[O6C"   _nave_sel_word_right   # Ctrl+Shift+Right (app short)
bindkey "^[[1;4C" _nave_sel_word_right   # Alt+Shift+Right
bindkey "^[[1;6A" _nave_sel_up           # Ctrl+Shift+Up (multiline)
bindkey "^[[1;6B" _nave_sel_down         # Ctrl+Shift+Down (multiline)
bindkey "^[[1;2H" _nave_sel_home         # Shift+Home
bindkey "^[[1;6H" _nave_sel_home         # Ctrl+Shift+Home
bindkey "^[[1;2F" _nave_sel_end          # Shift+End
bindkey "^[[1;6F" _nave_sel_end          # Ctrl+Shift+End
bindkey "^A"      _nave_select_all       # Ctrl+A (Select All)
bindkey "^L"      _nave_select_all       # Ctrl+L (Select current line, VS Code)
bindkey "^D"      _nave_select_word      # Ctrl+D (Select word; 2x expands to full arg; never exits shell)

# Autocomplete (VS Code: Ctrl+Space) & Multiline (Shift+Enter / Alt+Enter)
if (( ${+widgets[fzf-completion]} )); then
  bindkey "^@" fzf-completion            # Ctrl+Space (fzf + Zsh completion)
else
  bindkey "^@" expand-or-complete        # Ctrl+Space (Zsh completion)
fi
bindkey "\e\r" _nave_insert_newline      # Shift+Enter / Alt+Enter (newline without executing)
bindkey "\e\n" _nave_insert_newline      # Alt+Ctrl+J fallback
bindkey "\e\e" _nave_sudo_toggle         # Esc Esc (Toggle sudo prefix)

# Editing & Deletion
bindkey "^?"        _nave_backspace      # Backspace
bindkey "^[[3~"     _nave_delete         # Delete
bindkey "^H"        _nave_kill_word_back # Ctrl+Backspace (WT / VS Code)
bindkey "^W"        _nave_kill_word_back # Ctrl+W
bindkey "^[^?"      _nave_kill_word_back # Alt+Backspace
bindkey "^[[127;5u" _nave_kill_word_back # Ctrl+Backspace (CSI u)
bindkey "^[[3;5~"   _nave_kill_word_fwd  # Ctrl+Delete
bindkey "^[d"       _nave_kill_word_fwd  # Alt+D
bindkey "^U"        backward-kill-line   # Ctrl+U (Delete to start of line)
bindkey "^[[127;6u" backward-kill-line   # Ctrl+Shift+Backspace (CSI u)
bindkey "^K"        kill-line            # Ctrl+K (Delete to end of line)
bindkey "^[[3;6~"   kill-line            # Ctrl+Shift+Delete

# Undo / Redo (VS Code: Ctrl+Z, Ctrl+Y, Ctrl+Shift+Z)
bindkey "^Z"        undo                 # Ctrl+Z
bindkey "^Y"        redo                 # Ctrl+Y
bindkey "^[[90;6u"  redo                 # Ctrl+Shift+Z (CSI u via WT)
bindkey "^[[122;6u" redo                 # Ctrl+Shift+Z (lowercase CSI u)

# Cut / Copy / Line Actions
bindkey -rp "^X" 2>/dev/null             # Remove all ^X prefix bindings so Ctrl+X fires instantly
bindkey "^X"       _nave_cut             # Ctrl+X (Cut selection or line -> Windows clipboard)
bindkey "^[w"      _nave_copy            # Alt+W (Copy selection or line -> Windows clipboard)
bindkey "^[[2;5~"  _nave_copy            # Ctrl+Insert (Copy selection or line -> Windows clipboard)
bindkey "^[[75;6u" _nave_delete_line     # Ctrl+Shift+K (Delete whole line)
bindkey "^[[68;6u" _nave_duplicate       # Ctrl+Shift+D (Duplicate selection or line)
bindkey "^[[1;4B"  _nave_duplicate       # Shift+Alt+Down (Duplicate line/selection, VS Code native)
bindkey "^[[1;4A"  _nave_duplicate       # Shift+Alt+Up (Duplicate line/selection, VS Code native)
bindkey "^_"       _nave_toggle_comment  # Ctrl+/ (Toggle # comment)
bindkey "^[[47;5u" _nave_toggle_comment  # Ctrl+; ABNT2 / Ctrl+/ US (CSI u via WT)
bindkey "^[l"      clear-screen          # Alt+L (Clear screen since Ctrl+L selects line)
bindkey "^[e"      edit-command-line     # Alt+E (Open command in $EDITOR)
