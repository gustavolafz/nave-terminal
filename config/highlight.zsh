# ==============================================================================
# ~/.config/nave/highlight.zsh
# nave-highlight — Pure Zsh 5.9 real-time syntax highlighter (zero external plugins)
# Live-synced to active Nave Obsidian palette (_NAVE_C), <1ms per keystroke.
# ==============================================================================

autoload -Uz add-zle-hook-widget
zmodload zsh/parameter 2>/dev/null

typeset -g _NAVE_HL_ENABLED=1
[[ -r "${NAVE_CONFIG_DIR:-$HOME/.config/nave}/highlight" ]] && _NAVE_HL_ENABLED="$(<"${NAVE_CONFIG_DIR:-$HOME/.config/nave}/highlight")"
typeset -g _NAVE_HL_LAST_BUF=""
typeset -g _NAVE_HL_LAST_PAL=""

function nave-highlight {
  local cfg="${NAVE_CONFIG_DIR:-$HOME/.config/nave}/highlight"
  case "$1" in
    on|1)
      _NAVE_HL_ENABLED=1
      _NAVE_HL_LAST_BUF=""
      print -r -- "1" > "$cfg"
      print -P "%F{${_NAVE_C[ok]:-48}}nave-highlight ativado.%f"
      ;;
    off|0)
      _NAVE_HL_ENABLED=0
      _NAVE_HL_LAST_BUF=""
      print -r -- "0" > "$cfg"
      print -P "%F{${_NAVE_C[warn]:-214}}nave-highlight desativado.%f"
      ;;
    *)
      local st="on"
      (( _NAVE_HL_ENABLED )) || st="off"
      print -P "nave-highlight: %B${st}%b (uso: nave-highlight on|off)"
      ;;
  esac
}

function _nave_hl_apply_selection {
  # Zsh region_highlight overrides zle_highlight=(region:...), so syntax-colored
  # tokens (commands, flags, paths, strings) hide native region selection unless
  # we append an explicit selection overlay AFTER all memo=nave-hl entries.
  region_highlight=(${region_highlight:#*memo=nave-sel*})
  if (( REGION_ACTIVE )); then
    local s=$(( MARK < CURSOR ? MARK : CURSOR ))
    local e=$(( MARK > CURSOR ? MARK : CURSOR ))
    if (( e > s )); then
      region_highlight+=("${s} ${e} bg=238,fg=${_NAVE_C[user]:-51},bold, memo=nave-sel")
    fi
  fi
}

function _nave_hl_redraw {
  setopt LOCAL_OPTIONS EXTENDED_GLOB
  if (( ! _NAVE_HL_ENABLED )); then
    region_highlight=(${region_highlight:#*memo=nave-hl*})
    _nave_hl_apply_selection
    return
  fi

  local buf="$BUFFER"
  local pal="${NAVE_PALETTE:-cyber}"
  if [[ "$buf" == "$_NAVE_HL_LAST_BUF" && "$pal" == "$_NAVE_HL_LAST_PAL" ]]; then
    _nave_hl_apply_selection
    return
  fi
  _NAVE_HL_LAST_BUF="$buf"
  _NAVE_HL_LAST_PAL="$pal"

  # Preserve other highlights (e.g. nave-suggest POSTDISPLAY memo=nave-sg)
  # Note: Zsh normalizes ", memo=..." internally to " memo=...", so match *memo=nave-hl*!
  local -a kept=(${region_highlight:#*memo=nave-hl*})
  kept=(${kept:#*memo=nave-sel*})
  if [[ -z "$buf" ]]; then
    region_highlight=("${kept[@]}")
    return
  fi

  local c_ok="${_NAVE_C[ok]:-48}"
  local c_usr="${_NAVE_C[user]:-51}"
  local c_err="${_NAVE_C[err]:-197}"
  local c_git="${_NAVE_C[git]:-171}"
  local c_warn="${_NAVE_C[warn]:-214}"
  local c_info="${_NAVE_C[info]:-51}"
  local c_path="${_NAVE_C[path]:-87}"
  local c_str="${_NAVE_C[str]:-220}"
  local c_mut="${_NAVE_C[muted]:-244}"

  local -a tokens
  tokens=(${(Z+c+)buf})

  local -a hl
  local pos=0 len=$#buf expect_cmd=1 tok tlen start end style expanded

  for tok in "${tokens[@]}"; do
    # Skip leading horizontal spaces/tabs
    while (( pos < len )) && [[ "${buf:$pos:1}" == [[:blank:]] ]]; do
      (( pos++ ))
    done

    # Handle explicit newline tokens from (Z+n+)
    if [[ "$tok" == $'\n' ]]; then
      while (( pos < len )) && [[ "${buf:$pos:1}" == $'\n' ]]; do
        (( pos++ ))
      done
      expect_cmd=1
      continue
    fi

    tlen=$#tok
    if [[ "${buf:$pos:$tlen}" != "$tok" ]]; then
      # Fallback scan forward if whitespace/escape shifted offset
      local rel="${buf:$pos}"
      local idx="${rel[(I)$tok]}"
      if (( idx > 0 )); then
        (( pos += idx - 1 ))
      else
        continue
      fi
    fi

    start=$pos
    end=$(( pos + tlen ))
    pos=$end
    style=""

    # 1. Comments (# ...)
    if [[ "$tok" == \#* ]]; then
      style="fg=${c_mut}"
    # 2. Shell control operators / redirections
    elif [[ "$tok" == (\||\|\||\&\&|\;|\&|\|\&|\(|\)|\{|\}|\;\;|\;\&) ]]; then
      style="fg=${c_git},bold"
      [[ "$tok" != (")"|"}") ]] && expect_cmd=1
    elif [[ "$tok" == (\>|\>\>|\<|\<\<|\<\<\<|[0-9]\>|[0-9]\>\>|[0-9]\>\&[0-9]|\&\>|\&\>\>) ]]; then
      style="fg=${c_git},bold"
    # 3. Command position
    elif (( expect_cmd )); then
      if [[ "$tok" == [A-Za-z_][A-Za-z0-9_]#=* ]]; then
        # Env var assignment: KEY=VAL (command still expected next)
        local eq_off="${tok[(i)=]}"
        hl+=("${start} $(( start + eq_off - 1 )) fg=${c_git}, memo=nave-hl")
        if (( eq_off < tlen )); then
          hl+=("$(( start + eq_off )) ${end} fg=${c_str}, memo=nave-hl")
        fi
        continue
      elif [[ "$tok" == (sudo|command|builtin|noglob|nocorrect|exec|time|env|nohup|nice|notify) ]]; then
        style="fg=${c_warn},bold"
        # expect_cmd remains 1 so the wrapped command is highlighted too!
      elif [[ "$tok" == (if|then|else|elif|fi|for|in|do|done|while|until|case|esac|function|select|\[\[|\]\]|\!) ]]; then
        style="fg=${c_git},bold"
        case "$tok" in
          if|then|else|elif|do|while|until|\!) expect_cmd=1 ;;
          *) expect_cmd=0 ;;
        esac
      elif (( ${+aliases[$tok]} || ${+functions[$tok]} || ${+builtins[$tok]} )); then
        style="fg=${c_usr},bold"
        expect_cmd=0
      elif (( ${+commands[$tok]} )); then
        style="fg=${c_ok},bold"
        expect_cmd=0
      else
        expanded="$tok"
        [[ "$expanded" == "~/"* ]] && expanded="${HOME}/${expanded#\~/}"
        if [[ -x "$expanded" && ! -d "$expanded" ]]; then
          style="fg=${c_ok},bold"
        elif [[ -d "$expanded" ]]; then
          style="fg=${c_path},underline"
        else
          style="fg=${c_err},bold"
        fi
        expect_cmd=0
      fi
    # 4. Argument position
    else
      if [[ "$tok" == \'*\' || "$tok" == \"*\" || "$tok" == \$\'*\' ]]; then
        style="fg=${c_str}"
      elif [[ "$tok" == \'* || "$tok" == \"* ]]; then
        # Unclosed quote highlight
        style="fg=${c_err}"
      elif [[ "$tok" == -* && "$tok" != "-" ]]; then
        style="fg=${c_info}"
      elif [[ "$tok" == \$* ]]; then
        style="fg=${c_git}"
      elif [[ "$tok" == *[\*\?]* ]]; then
        style="fg=${c_warn}"
      elif [[ "$tok" != *"://"* ]]; then
        expanded="$tok"
        [[ "$expanded" == "~" ]] && expanded="$HOME"
        [[ "$expanded" == "~/"* ]] && expanded="${HOME}/${expanded#\~/}"
        [[ -e "$expanded" ]] && style="fg=${c_path},underline"
      fi
    fi

    [[ -n "$style" ]] && hl+=("${start} ${end} ${style}, memo=nave-hl")
  done

  region_highlight=("${kept[@]}" "${hl[@]}")
  _nave_hl_apply_selection
}

function _nave_hl_finish {
  _NAVE_HL_LAST_BUF=""
  region_highlight=(${region_highlight:#*memo=nave-sel*})
}

add-zle-hook-widget zle-line-pre-redraw _nave_hl_redraw
add-zle-hook-widget zle-line-finish _nave_hl_finish
