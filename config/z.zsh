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

# Populates `reply` array directly in-shell (zero subshells/forks, ~10x faster!)
function _nave_z_ranked {
  reply=()
  [[ -r "$NAVE_Z_DB" ]] || return 1
  local now=$EPOCHSECONDS
  local s t p age rank padded
  local -a rows=()
  while IFS='|' read -r s t p; do
    [[ -d "$p" ]] || continue
    age=$(( now - ${t:-$now} ))
    (( age < 0 )) && age=0
    local si=${s%%.*}
    (( si < 1 )) && si=1
    if (( age < 3600 )); then
      rank=$(( si * 400 ))
    elif (( age < 86400 )); then
      rank=$(( si * 200 ))
    elif (( age < 604800 )); then
      rank=$(( si * 100 ))
    else
      rank=$(( si * 50 ))
    fi
    padded="${(l:10::0:)rank}"
    rows+=("${padded}|${p}")
  done < "$NAVE_Z_DB"

  (( ${#rows} )) || return 1
  reply=("${(@On)rows}")
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
      local -a reply=()
      _nave_z_ranked
      for row in "${reply[@]}"; do
        rk="${row%%|*}"
        path_str="${row#*|}"
        [[ -n "$path_str" ]] || continue
        printf "  \e[38;5;%sm%5d\e[0m  %s\n" "$c_ok" "$(( 10#$rk / 100 ))" "${path_str/#$HOME/~}"
        if (( ++shown >= 20 )); then break; fi
      done
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
  local -a terms=("$@") reply=()
  _nave_z_ranked
  local best_base="" best_any="" row p lower_p lower_base ok t rem
  for row in "${reply[@]}"; do
    p="${row#*|}"
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
  done

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
  local -a dirs=() reply=()
  _nave_z_ranked
  local row p
  for row in "${reply[@]}"; do
    p="${row#*|}"
    [[ -n "$p" ]] && dirs+=("$p")
  done

  if (( ${#dirs} == 0 )); then
    print -P "%F{${_NAVE_C[warn]:-214}}nave-z: historico ainda vazio.%f"
    return 1
  fi

  local choice=""
  if command -v fzf >/dev/null 2>&1; then
    choice=$(printf '%s\n' "${dirs[@]}" | fzf --query="$query" --header="nave-z — Ir para diretorio" --preview 'eza --color=always --group-directories-first {} 2>/dev/null || ls -la --color=always {} 2>/dev/null | head -n 40')
  else
    local PS3="Escolha o numero (Enter cancela): "
    select choice in "${dirs[@]:0:20}"; do
      break
    done
  fi
  [[ -n "$choice" ]] && builtin cd "$choice"
}

# Native ranked Tab completion for `z` and `zi` (ordered by frecency score)
function _nave_comp_z {
  local -a reply=() cands=() disp=()
  _nave_z_ranked || return 1
  local row rk p sc
  for row in "${reply[@]}"; do
    rk="${row%%|*}"
    p="${row#*|}"
    [[ -n "$p" && "$p" != "$PWD" ]] || continue
    sc=$(( 10#$rk / 100 ))
    cands+=("$p")
    disp+=("$(printf '%4d  %s' "$sc" "${p/#$HOME/~}")")
    (( ${#cands} >= 40 )) && break
  done
  (( ${#cands} )) && compadd -V nave-z -d disp -Q -- "${cands[@]}"
}
compdef _nave_comp_z z zi 2>/dev/null
