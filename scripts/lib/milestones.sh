# ── Milestone management ──────────────────────────────────────────
# Depende de: github.sh

sync_milestones() {
  local repo
  repo="$(github_repo)"
  local milestones_file="${ROADMAP_DIR:-roadmap}/milestones.yaml"

  if [[ ! -f "$milestones_file" ]]; then
    warn "milestones.yaml no encontrado en $milestones_file"
    return
  fi

  info "Sincronizando milestones con $repo ..."

  # Cachear milestones existentes
  local existing_json
  existing_json="$($GH api "/repos/$repo/milestones" --paginate 2>/dev/null)"
  local existing_titles=()
  while IFS= read -r t; do
    [[ -n "$t" ]] && existing_titles+=("$t")
  done < <(echo "$existing_json" | jq -r '.[].title // empty' 2>/dev/null)

  local current_title="" current_desc="" current_due="" current_state="open"
  local in_entry=false reading_desc=false
  local created=0 updated=0

  while IFS= read -r line; do
    if [[ "$line" =~ ^[[:space:]]*-[[:space:]]*title:[[:space:]]*\"(.+)\" ]]; then
      # Procesar anterior
      if $in_entry && [[ -n "$current_title" ]]; then
        _ms_upsert "$repo" "$current_title" "$current_desc" "$current_due" \
                   "$current_state" existing_titles created updated
      fi
      current_title="${BASH_REMATCH[1]}"
      current_desc=""; current_due=""; current_state="open"
      in_entry=true; reading_desc=false

    elif $in_entry && [[ "$line" =~ ^[[:space:]]+description:[[:space:]]*\"(.*)\" ]]; then
      current_desc="${BASH_REMATCH[1]}"
    elif $in_entry && [[ "$line" =~ ^[[:space:]]+description:[[:space:]]*\|\s*$ ]]; then
      reading_desc=true; current_desc=""
    elif $in_entry && $reading_desc; then
      if [[ "$line" =~ ^[[:space:]]{6} ]]; then
        local text="${line#"${line%%[! ]*}"}"
        current_desc+="$text"$'\n'
      else
        reading_desc=false
      fi
    elif $in_entry && [[ "$line" =~ ^[[:space:]]+due_on:[[:space:]]*\"(.+)\" ]]; then
      current_due="${BASH_REMATCH[1]}"
    elif $in_entry && [[ "$line" =~ ^[[:space:]]+state:[[:space:]]*\"?(open|closed)\"? ]]; then
      current_state="${BASH_REMATCH[1]}"
    fi
  done < "$milestones_file"

  if $in_entry && [[ -n "$current_title" ]]; then
    _ms_upsert "$repo" "$current_title" "$current_desc" "$current_due" \
               "$current_state" existing_titles created updated
  fi

  ok "Milestones: $created creados, $updated actualizados"
}

_ms_upsert() {
  local repo="$1" title="$2" desc="$3" due="$4" state="$5"
  local -n _titles="$6"
  local -n _created="$7"
  local -n _updated="$8"

  local exists=false
  for t in "${_titles[@]}"; do
    if [[ "$t" == "$title" ]]; then
      exists=true
      break
    fi
  done

  local data
  data="$(jq -n \
    --arg title "$title" \
    --arg desc "$desc" \
    --arg state "$state" \
    '{title: $title, description: $desc, state: $state}')"

  [[ -n "$due" ]] && data="$(echo "$data" | jq --arg due "$due" '.due_on = $due')"

  if $exists; then
    local encoded
    encoded="$(jq -rn --arg t "$title" '$t|@uri')"
    if $GH api "/repos/$repo/milestones/$encoded" --method PATCH \
      --input - <<<"$data" &>/dev/null; then
      ((_updated++))
    fi
  else
    if $GH api "/repos/$repo/milestones" --input - <<<"$data" &>/dev/null; then
      ((_created++))
    fi
  fi
}
