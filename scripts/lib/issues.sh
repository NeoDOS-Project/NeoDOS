# ── Issue management ──────────────────────────────────────────────
# Depende de: github.sh
# Parsea roadmap/improvements.md y sincroniza con GitHub Issues.
#
# Formato esperado en improvements.md:
#   - **ID**: Título `prio` `label1` `label2` `milestone`
#     Descripción del issue.
#     state: open|closed
#     Dependencies: lista

sync_issues() {
  local repo
  repo="$(github_repo)"
  local md_file="${ROADMAP_DIR:-roadmap}/improvements.md"

  if [[ ! -f "$md_file" ]]; then
    warn "improvements.md no encontrado en $md_file"
    return
  fi

  info "Sincronizando issues con $repo ..."

  # Cachear todas las issues existentes
  local all_issues_json
  all_issues_json="$($GH api "/repos/$repo/issues?state=all&per_page=100" --paginate 2>/dev/null)"

  local created=0 updated=0 skipped=0 closed=0
  local in_item=false
  local current_id="" current_title="" current_body=""
  local prio="" milestone="" labels="" state="open" deps=""

  while IFS= read -r line; do
    # Detectar inicio de item: - **ID**: Título `backtick` `tokens`
    if [[ "$line" =~ ^[[:space:]]*-[[:space:]]*\*\*([A-Za-z0-9_.-]+)\*\*:[[:space:]]*(.+)$ ]]; then
      # Procesar item anterior
      if [[ -n "$current_id" ]]; then
        _issue_upsert "$repo" "$current_id" "$current_title" "$prio" "$labels" \
                      "$milestone" "$current_body" "$state" "$deps" \
                      "$all_issues_json" created updated skipped closed
      fi

      current_id="${BASH_REMATCH[1]}"
      local title_raw="${BASH_REMATCH[2]}"
      current_body=""; prio=""; milestone=""; labels=""; state="open"; deps=""
      in_item=true

      # Extraer tokens backtick
      local tokens=() token
      local tmp="$title_raw"
      while [[ "$tmp" =~ \`([^\`]+)\` ]]; do
        tokens+=("${BASH_REMATCH[1]}")
        tmp="${tmp#*"${BASH_REMATCH[0]}"}"
      done

      for token in "${tokens[@]}"; do
        if [[ "$token" == priority/* ]]; then
          prio="$token"
        elif [[ "$token" =~ ^v[0-9] ]]; then
          milestone="$token"
        else
          [[ -z "$labels" ]] && labels="$token" || labels+=",$token"
        fi
      done

      # Título limpio
      local clean="$title_raw"
      for token in "${tokens[@]}"; do clean="${clean//\`$token\`/}"; done
      clean="$(echo "$clean" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
      current_title="$current_id: $clean"

    elif $in_item && [[ "$line" =~ ^[[:space:]]+state:[[:space:]]*(open|closed) ]]; then
      state="${BASH_REMATCH[1]}"
    elif $in_item && [[ "$line" =~ ^[[:space:]]+Dependencies:[[:space:]]*(.+) ]]; then
      deps="${BASH_REMATCH[1]}"
    elif $in_item && [[ "$line" =~ ^[[:space:]] ]] \
      && [[ -n "$(echo "$line" | sed 's/^[[:space:]]*//')" ]]; then
      local trimmed="$(echo "$line" | sed 's/^[[:space:]]*//')"
      [[ -n "$current_body" ]] && current_body+=$'\n'
      current_body+="$trimmed"
    fi
  done < "$md_file"

  # Último item
  if [[ -n "$current_id" ]]; then
    _issue_upsert "$repo" "$current_id" "$current_title" "$prio" "$labels" \
                  "$milestone" "$current_body" "$state" "$deps" \
                  "$all_issues_json" created updated skipped closed
  fi

  ok "Issues: $created creadas, $updated actualizadas, $closed cerradas, $skipped omitidas (sin cambios)"
}

_issue_upsert() {
  local repo="$1" id="$2" title="$3" prio="$4" labels_arg="$5"
  local milestone_title="$6" body="$7" state="$8" deps="$9"
  local issues_json="${10}"
  local -n _created="${11}" _updated="${12}" _skipped="${13}" _closed="${14}"

  # Body completo
  local full_body="$body"
  [[ -n "$deps" ]] && full_body+=$'\n\n## Dependencias\n'"$deps"
  full_body+=$'\n\n---\n> Sincronizado desde `roadmap/improvements.md`'

  # Buscar issue existente por título
  local existing_num="" existing_state=""
  if [[ -n "$issues_json" && "$issues_json" != "null" ]]; then
    while IFS= read -r item; do
      [[ -z "$item" ]] && continue
      local inum ititle istate
      inum="$(echo "$item" | jq -r '.number // ""')"
      ititle="$(echo "$item" | jq -r '.title // ""')"
      istate="$(echo "$item" | jq -r '.state // ""')"
      if [[ "$ititle" == "$title" ]]; then
        existing_num="$inum"; existing_state="$istate"
        break
      fi
    done < <(echo "$issues_json" | jq -c '.[] | {number, title, state}' 2>/dev/null)
  fi

  if [[ -n "$existing_num" ]]; then
    # ── Ya existe ──
    if [[ "$state" == "closed" && "$existing_state" != "closed" ]]; then
      $GH api "/repos/$repo/issues/$existing_num" --method PATCH \
        -f state="closed" &>/dev/null && { ((_closed++)); ((_updated++)); }
    elif [[ "$state" == "open" && "$existing_state" == "closed" ]]; then
      $GH api "/repos/$repo/issues/$existing_num" --method PATCH \
        -f state="open" &>/dev/null && ((_updated++))
    else
      ((_skipped++))
    fi
  else
    # ── Crear ──
    local data="{}"
    data="$(echo "$data" | jq \
      --arg t "$title" --arg b "$full_body" '.title=$t | .body=$b')"

    # Labels
    local all_labels="$prio"
    [[ -n "$labels_arg" ]] && all_labels+=",$labels_arg"
    all_labels="$(echo "$all_labels" | sed 's/^,//;s/,$//')"
    if [[ -n "$all_labels" ]]; then
      local arr="[]"
      local IFS=','
      for l in $all_labels; do
        l="$(echo "$l" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
        [[ -z "$l" ]] && continue
        arr="$(echo "$arr" | jq --arg l "$l" '. + [$l]')"
      done
      data="$(echo "$data" | jq --argjson labels "$arr" '.labels=$labels')"
    fi

    # Milestone number
    if [[ -n "$milestone_title" ]]; then
      local ms_num
      ms_num="$($GH api "/repos/$repo/milestones?per_page=100" --paginate \
        --jq ".[] | select(.title == \"$milestone_title\") | .number" 2>/dev/null | head -1)"
      [[ -n "$ms_num" ]] && data="$(echo "$data" | jq --argjson ms "$ms_num" '.milestone=$ms')"
    fi

    local result
    result="$($GH api "/repos/$repo/issues" --input - <<<"$data" 2>/dev/null)" || {
      warn "Error creando issue: $title"
      return
    }
    ((_created++))

    # Cerrar si state=closed
    if [[ "$state" == "closed" ]]; then
      local new_num
      new_num="$(echo "$result" | jq -r '.number // ""')"
      [[ -n "$new_num" ]] && $GH api "/repos/$repo/issues/$new_num" --method PATCH \
        -f state="closed" &>/dev/null && ((_closed++))
    fi
  fi
}
