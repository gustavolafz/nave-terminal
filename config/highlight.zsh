# ==============================================================================
# ~/.config/nave/highlight.zsh
# nave-highlight v2 — Pure Zsh 5.9 real-time syntax highlighter (zero external plugins)
# Live-synced to active Nave palette (_NAVE_C), <0.5ms per keystroke.
# Features:
#   - Cached command resolution (never triggers full /mnt/c PATH scan on WSL2)
#   - Literal token offset matching (safe on repeated tokens, globs, brackets)
#   - Destructive command warning (rm -rf, dd, mkfs, git reset --hard, push -f)
#   - Subcommand highlighting (git, docker, cargo, npm, pnpm, systemctl)
#   - --flag=value split styling, URLs, numbers, "$VAR" inside double quotes
#   - Unmatched delimiter error highlighting ((), [], {})
# ==============================================================================

autoload -Uz add-zle-hook-widget
zmodload zsh/parameter 2>/dev/null

typeset -g _NAVE_HL_ENABLED=1
[[ -r "${NAVE_CONFIG_DIR:-$HOME/.config/nave}/highlight" ]] && _NAVE_HL_ENABLED="$(<"${NAVE_CONFIG_DIR:-$HOME/.config/nave}/highlight")"
typeset -g _NAVE_HL_LAST_BUF=""
typeset -g _NAVE_HL_LAST_PAL=""
typeset -g _NAVE_HL_CACHE_PATH=""
typeset -gA _NAVE_HL_CMD_CACHE=()

function nave-highlight {
  local cfg="${NAVE_CONFIG_DIR:-$HOME/.config/nave}/highlight"
  case "$1" in
    on|1)
      _NAVE_HL_ENABLED=1
      _NAVE_HL_LAST_BUF=""
      print -r -- "1" >| "$cfg"
      print -P "%F{${_NAVE_C[ok]:-48}}✓ nave-highlight ativado.%f"
      ;;
    off|0)
      _NAVE_HL_ENABLED=0
      _NAVE_HL_LAST_BUF=""
      print -r -- "0" >| "$cfg"
      print -P "%F{${_NAVE_C[warn]:-214}}nave-highlight desativado.%f"
      ;;
    *)
      local st="on"
      (( _NAVE_HL_ENABLED )) || st="off"
      print -P "nave-highlight: %B${st}%b (uso: nave-highlight on|off)"
      ;;
  esac
}

function _nave_hl_cmd_exists {
  local cmd="$1"
  if [[ "$_NAVE_HL_CACHE_PATH" != "$PATH" ]]; then
    _NAVE_HL_CMD_CACHE=()
    _NAVE_HL_CACHE_PATH="$PATH"
  fi
  if (( ${+_NAVE_HL_CMD_CACHE[$cmd]} )); then
    return ${_NAVE_HL_CMD_CACHE[$cmd]}
  fi
  if whence -p -- "$cmd" >/dev/null 2>&1; then
    _NAVE_HL_CMD_CACHE[$cmd]=0
    return 0
  else
    _NAVE_HL_CMD_CACHE[$cmd]=1
    return 1
  fi
}

function _nave_hl_apply_selection {
  # Zsh region_highlight overrides zle_highlight=(region:...), so syntax-colored
  # tokens hide native region selection unless we append a selection overlay last.
  region_highlight=(${region_highlight:#*memo=nave-sel*})
  if (( REGION_ACTIVE )); then
    local pre=$#PREDISPLAY
    local s=$(( pre + (MARK < CURSOR ? MARK : CURSOR) ))
    local e=$(( pre + (MARK > CURSOR ? MARK : CURSOR) ))
    if (( e > s )); then
      region_highlight+=("${s} ${e} bg=238,fg=${_NAVE_C[user]:-51},bold, memo=nave-sel")
    fi
  fi
}

function _nave_hl_redraw {
  emulate -L zsh
  setopt EXTENDED_GLOB NO_BEEP

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
  local -a kept=(${region_highlight:#*memo=nave-hl*})
  kept=(${kept:#*memo=nave-sel*})
  if [[ -z "$buf" ]]; then
    region_highlight=("${kept[@]}")
    return
  fi

  local pre=$#PREDISPLAY
  local c_ok="${_NAVE_C[ok]:-48}"
  local c_usr="${_NAVE_C[user]:-51}"
  local c_err="${_NAVE_C[err]:-197}"
  local c_git="${_NAVE_C[git]:-171}"
  local c_warn="${_NAVE_C[warn]:-214}"
  local c_info="${_NAVE_C[info]:-51}"
  local c_path="${_NAVE_C[path]:-87}"
  local c_str="${_NAVE_C[str]:-220}"
  local c_dur="${_NAVE_C[dur]:-220}"
  local c_mut="${_NAVE_C[muted]:-244}"

  local -a tokens
  tokens=(${(Z+c+)buf})

  local -a hl
  local pos=0 len=$#buf expect_cmd=1 tok tlen start end style expanded
  local cur_cmd="" cur_sub="" cmd_start=0 cmd_end=0 is_danger_cmd=0

  for tok in "${tokens[@]}"; do
    while (( pos < len )) && [[ "${buf:$pos:1}" == [[:blank:]] ]]; do
      (( pos++ ))
    done

    if [[ "$tok" == $'\n' ]]; then
      while (( pos < len )) && [[ "${buf:$pos:1}" == $'\n' ]]; do
        (( pos++ ))
      done
      expect_cmd=1
      cur_cmd=""
      cur_sub=""
      is_danger_cmd=0
      continue
    fi

    tlen=$#tok
    if [[ "${buf:$pos:$tlen}" != "$tok" ]]; then
      # Literal FIRST match forward (fixes glob & repeated-token offset bugs)
      local rel="${buf:$pos}"
      local idx="${rel[(i)${(b)tok}]}"
      if (( idx > 0 && idx <= $#rel )); then
        (( pos += idx - 1 ))
      else
        continue
      fi
    fi

    start=$(( pre + pos ))
    end=$(( pre + pos + tlen ))
    (( pos += tlen ))
    style=""

    # 1. Comments (# ...)
    if [[ "$tok" == \#* ]]; then
      style="fg=${c_mut}"
    # 2. Shell control operators / redirections
    elif [[ "$tok" == (\||\|\||\&\&|\;|\&|\|\&|\(|\)|\{|\}|\;\;|\;\&) ]]; then
      style="fg=${c_git},bold"
      if [[ "$tok" != (")"|"}") ]]; then
        expect_cmd=1
        cur_cmd=""
        cur_sub=""
        is_danger_cmd=0
      fi
    elif [[ "$tok" == (\>|\>\>|\<|\<\<|\<\<\<|[0-9]\>|[0-9]\>\>|[0-9]\>\&[0-9]|\&\>|\&\>\>) ]]; then
      style="fg=${c_git},bold"
    # 3. Command position
    elif (( expect_cmd )); then
      if [[ "$tok" == [A-Za-z_][A-Za-z0-9_]#=* ]]; then
        local eq_off="${tok[(i)=]}"
        hl+=("${start} $(( start + eq_off - 1 )) fg=${c_git}, memo=nave-hl")
        if (( eq_off < tlen )); then
          hl+=("$(( start + eq_off )) ${end} fg=${c_str}, memo=nave-hl")
        fi
        continue
      elif [[ "$tok" == (sudo|command|builtin|noglob|nocorrect|exec|time|env|nohup|nice|notify) ]]; then
        style="fg=${c_warn},bold"
      elif [[ "$tok" == (if|then|else|elif|fi|for|in|do|done|while|until|case|esac|function|select|\[\[|\]\]|\!) ]]; then
        style="fg=${c_git},bold"
        case "$tok" in
          if|then|else|elif|do|while|until|\!) expect_cmd=1 ;;
          *) expect_cmd=0 ;;
        esac
      elif [[ "$tok" == (dd|mkfs|mkfs.*|shred|wipefs|fdisk|parted) ]]; then
        style="fg=${c_err},bold,underline"
        cur_cmd="$tok"
        cur_sub=""
        is_danger_cmd=1
        expect_cmd=0
      elif (( ${+aliases[$tok]} || ${+functions[$tok]} || ${+builtins[$tok]} )); then
        style="fg=${c_usr},bold"
        cur_cmd="$tok"
        cur_sub=""
        cmd_start=$start
        cmd_end=$end
        is_danger_cmd=0
        expect_cmd=0
      elif _nave_hl_cmd_exists "$tok"; then
        style="fg=${c_ok},bold"
        cur_cmd="$tok"
        cur_sub=""
        cmd_start=$start
        cmd_end=$end
        is_danger_cmd=0
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
        cur_cmd="$tok"
        cur_sub=""
        expect_cmd=0
      fi
    # 4. Argument position
    else
      # Detect destructive combinations: `rm -rf`, `git reset --hard`, `git push -f/--force`
      if [[ "$cur_cmd" == "rm" && "$tok" == (-*r*f*|-*f*r*|--no-preserve-root) ]] \
         || [[ "$cur_cmd" == "git" && "$cur_sub" == "reset" && "$tok" == "--hard" ]] \
         || [[ "$cur_cmd" == "git" && "$cur_sub" == "push" && "$tok" == (-f|--force|--force-with-lease) ]]; then
        (( cmd_end > cmd_start )) && hl+=("${cmd_start} ${cmd_end} fg=${c_err},bold,underline, memo=nave-hl")
        style="fg=${c_err},bold,underline"
        is_danger_cmd=1
      elif [[ -z "$cur_sub" && "$tok" != -* && "$cur_cmd" == (git|docker|cargo|npm|pnpm|systemctl|apt|pip|pip3) ]]; then
        cur_sub="$tok"
        style="fg=${c_info},bold"
      elif [[ "$tok" == \"*\" ]]; then
        # Double-quoted string: base string color + inner $VAR / ${VAR} interpolation
        hl+=("${start} ${end} fg=${c_str}, memo=nave-hl")
        local si=1 slen=$tlen ch vstart vend
        while (( si < slen - 1 )); do
          ch="${tok:$si:1}"
          if [[ "$ch" == '\\' ]]; then
            (( si += 2 ))
            continue
          fi
          if [[ "$ch" == '$' ]]; then
            vstart=$(( start + si ))
            if [[ "${tok:$((si+1)):1}" == '{' ]]; then
              local close_b="${tok[$((si+2)),-1][(i)\}]}"
              if (( close_b > 0 && si + 1 + close_b <= slen )); then
                vend=$(( start + si + 1 + close_b ))
                hl+=("${vstart} ${vend} fg=${c_git},bold, memo=nave-hl")
                (( si += 1 + close_b ))
                continue
              fi
            elif [[ "${tok:$((si+1)):1}" == [A-Za-z_@\#\?\$\!\*] ]]; then
              local vi=$(( si + 1 ))
              if [[ "${tok:$vi:1}" == [A-Za-z_] ]]; then
                while (( vi < slen - 1 )) && [[ "${tok:$vi:1}" == [A-Za-z0-9_] ]]; do
                  (( vi++ ))
                done
              else
                (( vi++ ))
              fi
              vend=$(( start + vi ))
              hl+=("${vstart} ${vend} fg=${c_git},bold, memo=nave-hl")
              si=$vi
              continue
            fi
          fi
          (( si++ ))
        done
        continue
      elif [[ "$tok" == \'*\' || "$tok" == \$\'*\' ]]; then
        style="fg=${c_str}"
      elif [[ "$tok" == \'* || "$tok" == \"* ]]; then
        style="fg=${c_err},underline"
      elif [[ "$tok" == --[A-Za-z0-9_-]#=* ]]; then
        # Split `--flag=value` into distinct flag / operator / value colors
        local eq_off="${tok[(i)=]}"
        hl+=("${start} $(( start + eq_off - 1 )) fg=${c_info}, memo=nave-hl")
        hl+=("$(( start + eq_off - 1 )) $(( start + eq_off )) fg=${c_git}, memo=nave-hl")
        if (( eq_off < tlen )); then
          hl+=("$(( start + eq_off )) ${end} fg=${c_str}, memo=nave-hl")
        fi
        continue
      elif [[ "$tok" == -* && "$tok" != "-" ]]; then
        style="fg=${c_info}"
      elif [[ "$tok" == (http://*|https://*|ssh://*|ftp://*|git@*:*) ]]; then
        style="fg=${c_info},underline"
      elif [[ "$tok" == \$* ]]; then
        style="fg=${c_git}"
      elif [[ "$tok" == [0-9]## || "$tok" == 0[xX][0-9a-fA-F]## ]]; then
        style="fg=${c_dur}"
      elif [[ "$tok" == *[\*\?]* ]]; then
        style="fg=${c_warn}"
      else
        expanded="$tok"
        [[ "$expanded" == "~" ]] && expanded="$HOME"
        [[ "$expanded" == "~/"* ]] && expanded="${HOME}/${expanded#\~/}"
        [[ -e "$expanded" ]] && style="fg=${c_path},underline"
      fi
    fi

    [[ -n "$style" ]] && hl+=("${start} ${end} ${style}, memo=nave-hl")
  done

  # 5. Lightweight unmatched bracket/paren/brace check outside quotes/comments
  local -a dstack=()
  local i=0 in_sq=0 in_dq=0 esc=0 ch top_ch
  while (( i < len )); do
    ch="${buf:$i:1}"
    if (( esc )); then
      esc=0
    elif (( in_sq )); then
      [[ "$ch" == "'" ]] && in_sq=0
    elif (( in_dq )); then
      if [[ "$ch" == '\\' ]]; then esc=1
      elif [[ "$ch" == '"' ]]; then in_dq=0
      fi
    else
      case "$ch" in
        \\) esc=1 ;;
        \') in_sq=1 ;;
        \") in_dq=1 ;;
        \#) (( i == 0 )) || [[ "${buf:$((i-1)):1}" == [[:space:]] ]] && break ;;
        \(|\{|\[) dstack+=("${ch}:${i}") ;;
        \)|\}|\])
          if (( ${#dstack} == 0 )); then
            hl+=("$(( pre + i )) $(( pre + i + 1 )) fg=${c_err},bold,underline, memo=nave-hl")
          else
            top_ch="${dstack[-1]%%:*}"
            if [[ ( "$ch" == ")" && "$top_ch" == "(" ) || ( "$ch" == "}" && "$top_ch" == "{" ) || ( "$ch" == "]" && "$top_ch" == "[" ) ]]; then
              dstack[-1]=()
            else
              hl+=("$(( pre + i )) $(( pre + i + 1 )) fg=${c_err},bold,underline, memo=nave-hl")
            fi
          fi
          ;;
      esac
    fi
    (( i++ ))
  done
  if (( ${#dstack} > 0 )); then
    local entry dpos
    for entry in "${dstack[@]}"; do
      dpos="${entry#*:}"
      hl+=("$(( pre + dpos )) $(( pre + dpos + 1 )) fg=${c_err},bold,underline, memo=nave-hl")
    done
  fi

  region_highlight=("${kept[@]}" "${hl[@]}")
  _nave_hl_apply_selection
}

function _nave_hl_finish {
  _NAVE_HL_LAST_BUF=""
  region_highlight=(${region_highlight:#*memo=nave-sel*})
}

add-zle-hook-widget zle-line-pre-redraw _nave_hl_redraw
add-zle-hook-widget zle-line-finish _nave_hl_finish
