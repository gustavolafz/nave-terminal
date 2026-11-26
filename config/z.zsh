# ==============================================================================
# ~/.config/nave/z.zsh
# nave-z — Pure Zsh frecency directory jumper (replaces zoxide, zero external deps)
# Commands: z [terms...], zi [query], z -l/--list, z --clean
# Database: ~/.local/share/nave/z.db  (format: score|timestamp|path)
# ==============================================================================

autoload -Uz add-zsh-hook
zmodload zsh/datetime 2>/dev/null

typeset -g NAVE_Z_DB="${NAVE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/nave}/z.db"
[[ -d "${NAVE_Z_DB:h}" ]] || mkdir -p "${NAVE_Z_DB:h}"

# One-time auto-import from zoxide if user had previous history and z.db is empty
if [[ ! -s "$NAVE_Z_DB" ]] && command -v zoxide >/dev/null 2>&1; then
  () {
    local now=$EPOCHSECONDS line score dir
    zoxide query -ls 2>/dev/null | while IFS= read -r line; do
      score="${${line## #}%% *}"
      dir="${line#*$score }"
      dir="${dir## #}"
      [[ -d "$dir" ]] && printf '%s|%s|%s\n' "${score:-1}" "$now" "$dir"
    done > "$NAVE_Z_DB"
  }
fi

# Record directory visits on every `cd` with time-decayed frecency score
function _nave_z_chpwd {
  local cur="$PWD"
  [[ "$cur" == "$HOME" || "$cur" == "/" || "$cur" == /tmp* ]] && return

  local now=$EPOCHSECONDS
  local -A scores times
  local line s t p

  if [[ -r "$NAVE_Z_DB" ]]; then
    while IFS='|' read -r s t p; do
      [[ -n "$p" ]] || continue
      scores[$p]="$s"
      times[$p]="$t"
    done < "$NAVE_Z_DB"
  fi

  local prev="${scores[$cur]:-0}"
  local prev_t="${times[$cur]:-$now}"
  local age=$(( now - prev_t ))
  (( age < 0 )) && age=0

  # Decay factor using pure integer math (half-life ~14 days = 1209600s, scale x100)
  local prev_x100=$(( ${prev%%.*} * 100 ))
  local decayed_x100=$(( prev_x100 * 1209600 / (1209600 + age) ))
  local next_score=$(( (decayed_x100 + 100) / 100 ))
  (( next_score < 1 )) && next_score=1

  scores[$cur]="$next_score"
  times[$cur]="$now"

  local tmp="${NAVE_Z_DB}.tmp.$$"
  {
    local count=0
    for p in "${(@k)scores}"; do
      printf '%s|%s|%s\n' "${scores[$p]}" "${times[$p]}" "$p"
      if (( ++count >= 500 )); then break; fi
    done
  } >| "$tmp" && command mv -f "$tmp" "$NAVE_Z_DB"
}

add-zsh-hook chpwd _nave_z_chpwd

function _nave_z_ranked {
  [[ -r "$NAVE_Z_DB" ]] || return 1
  local now=$EPOCHSECONDS
  local s t p age rank
  local -a rows
  while IFS='|' read -r s t p; do
    [[ -d "$p" ]] || continue
    age=$(( now - ${t:-$now} ))
    (( age < 0 )) && age=0
    # Frecency rank: higher score + recency bonus
    if (( age < 3600 )); then
      rank=$(( s * 400 ))
    elif (( age < 86400 )); then
      rank=$(( s * 200 ))
    elif (( age < 604800 )); then
      rank=$(( s * 100 ))
    else
      rank=$(( s * 50 ))
    fi
    rows+=("$(printf '%010d|%s' "$rank" "$p")")
  done < "$NAVE_Z_DB"

  (( ${#rows} )) || return 1
  printf '%s\n' "${(@On)rows}"
}

function z {
  if [[ $# -eq 0 ]]; then
    builtin cd ~
    return
  fi

  case "$1" in
    -)
      builtin cd -
      return
      ;;
    -l|--list)
      local c_dir="${_NAVE_C[dir]:-45}" c_mut="${_NAVE_C[muted]:-244}" c_ok="${_NAVE_C[ok]:-48}"
      print -P "%F{${c_dir}}╭─ nave-z — Diretorios frequentes ────────────────────────────%f"
      local row rk path_str shown=0
      while IFS='|' read -r rk path_str; do
        [[ -n "$path_str" ]] || continue
        printf "  \e[38;5;%sm%5d\e[0m  %s\n" "$c_ok" "$(( 10#$rk / 100 ))" "${path_str/#$HOME/~}"
        if (( ++shown >= 20 )); then break; fi
      done < <(_nave_z_ranked)
      (( shown == 0 )) && print -P "  %F{${c_mut}}(ainda sem historico — navegue com cd para alimentar o nave-z)%f"
      print -P "%F{${c_dir}}╰─────────────────────────────────────────────────────────────%f"
      return 0
      ;;
    --clean)
      [[ -r "$NAVE_Z_DB" ]] || return 0
      local tmp="${NAVE_Z_DB}.tmp.$$" removed=0 s t p
      : >| "$tmp"
      while IFS='|' read -r s t p; do
        if [[ -d "$p" ]]; then
          printf '%s|%s|%s\n' "$s" "$t" "$p" >> "$tmp"
        else
          (( removed++ ))
        fi
      done < "$NAVE_Z_DB"
      command mv -f "$tmp" "$NAVE_Z_DB"
      print -P "%F{${_NAVE_C[ok]:-48}}✓ nave-z: ${removed} diretorio(s) inexistente(s) removido(s).%f"
      return 0
      ;;
  esac

  # Direct existing directory path
  if [[ $# -eq 1 && -d "$1" ]]; then
    builtin cd "$1"
    return
  fi

  # Match all query terms in order (case-insensitive), prioritizing basename match + rank
  local -a terms=("$@")
  local best_base="" best_any="" row rk p lower_p lower_base ok t pos rem
  while IFS='|' read -r rk p; do
    [[ -n "$p" && "$p" != "$PWD" ]] || continue
    lower_p="${p:l}"
    rem="$lower_p"
    ok=1
    for t in "${terms[@]}"; do
      local lt="${t:l}"
      if [[ "$rem" != *"$lt"* ]]; then
        ok=0
        break
      fi
      rem="${rem#*$lt}"
    done
    (( ok )) || continue

    [[ -z "$best_any" ]] && best_any="$p"
    lower_base="${p:t:l}"
    if [[ -z "$best_base" && "$lower_base" == *"${terms[-1]:l}"* ]]; then
      best_base="$p"
      break
    fi
  done < <(_nave_z_ranked)

  local target="${best_base:-$best_any}"
  if [[ -n "$target" ]]; then
    builtin cd "$target"
  else
    print -P "%F{${_NAVE_C[warn]:-214}}nave-z: nenhum diretorio encontrado para '$*'.%f"
    return 1
  fi
}

function zi {
  local query="$*"
  local -a dirs
  local rk p
  while IFS='|' read -r rk p; do
    [[ -n "$p" ]] && dirs+=("$p")
  done < <(_nave_z_ranked)

  if (( ${#dirs} == 0 )); then
    print -P "%F{${_NAVE_C[warn]:-214}}nave-z: historico ainda vazio.%f"
    return 1
  fi

  local choice=""
  if command -v fzf >/dev/null 2>&1; then
    choice=$(printf '%s\n' "${dirs[@]}" | fzf --query="$query" --header="nave-z — Ir para diretorio" --preview 'ls -la --color=always {} 2>/dev/null | head -n 40')
  else
    # Pure Zsh fallback menu (zero external deps!)
    local PS3="Escolha o numero (Enter cancela): "
    select choice in "${dirs[@]:0:20}"; do
      break
    done
  fi
  [[ -n "$choice" ]] && builtin cd "$choice"
}
