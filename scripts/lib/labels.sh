# ── Label management ──────────────────────────────────────────────
# Depende de: github.sh

sync_labels() {
  local repo
  repo="$(github_repo)"
  local labels_file="${ROADMAP_DIR:-roadmap}/labels.yaml"

  if [[ ! -f "$labels_file" ]]; then
    warn "labels.yaml no encontrado en $labels_file"
    return
  fi

  info "Sincronizando labels con $repo ..."

  local existing
  existing="$($GH api "/repos/$repo/labels" --paginate --jq '[.[].name]' 2>/dev/null || echo '[]')"

  local current_name="" current_color="" current_desc=""
  local in_entry=false
  local created=0 updated=0

  while IFS= read -r line; do
    # Detectar comentarios y líneas vacías
    [[ "$line" =~ ^[[:space:]]*# ]] && continue
    [[ -z "$(echo "$line" | sed 's/^[[:space:]]*//')" ]] && continue

    # Inicio de entrada: - name: "value"
    if [[ "$line" =~ ^[[:space:]]*-[[:space:]]*name:[[:space:]]*\"?([^\"]+)\"?$ ]]; then
      # Procesar entrada anterior completa
      if $in_entry && [[ -n "$current_name" ]] && [[ -n "$current_color" ]]; then
        _label_upsert "$repo" "$current_name" "$current_color" "$current_desc" "$existing" created updated
      fi
      current_name="${BASH_REMATCH[1]}"
      current_color=""; current_desc=""
      in_entry=true

    elif $in_entry && [[ "$line" =~ ^[[:space:]]+color:[[:space:]]*\"?([^\"]+)\"?$ ]]; then
      current_color="${BASH_REMATCH[1]}"
    elif $in_entry && [[ "$line" =~ ^[[:space:]]+description:[[:space:]]*\"?([^\"]+)\"?$ ]]; then
      current_desc="${BASH_REMATCH[1]}"
    fi
  done < "$labels_file"

  # Última entrada
  if $in_entry && [[ -n "$current_name" ]] && [[ -n "$current_color" ]]; then
    _label_upsert "$repo" "$current_name" "$current_color" "$current_desc" "$existing" created updated
  fi

  ok "Labels: $created creados, $updated actualizados"
}

_label_upsert() {
  local repo="$1" name="$2" color="$3" desc="$4" existing="$5"
  local -n _created="$6" _updated="$7"

  if echo "$existing" | grep -qF "\"$name\"" 2>/dev/null; then
    $GH api "/repos/$repo/labels/$name" --method PATCH \
      -f color="$color" -f description="$desc" &>/dev/null && ((_updated++))
  else
    $GH api "/repos/$repo/labels" -f name="$name" \
      -f color="$color" -f description="$desc" &>/dev/null && ((_created++))
  fi
}
