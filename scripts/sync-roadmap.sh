#!/bin/bash
# ═══════════════════════════════════════════════════════════════════
# sync-roadmap.sh — Sincroniza el roadmap local con GitHub Issues
#
# Uso:
#   ./scripts/sync-roadmap.sh sync          # Sincroniza todo
#   ./scripts/sync-roadmap.sh labels        # Solo labels
#   ./scripts/sync-roadmap.sh milestones    # Solo milestones
#   ./scripts/sync-roadmap.sh issues        # Solo issues
#   ./scripts/sync-roadmap.sh changelog     # Genera changelog
#   ./scripts/sync-roadmap.sh check         # Verifica estado
#
# Configuración:
#   export GITHUB_REPOSITORY="owner/repo"   # Repo (auto-detecta si gh está configurado)
#
# Dependencias:
#   - gh (GitHub CLI) https://cli.github.com/
#   - Autenticación: gh auth login
# ═══════════════════════════════════════════════════════════════════

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROADMAP_DIR="$(cd "$SCRIPT_DIR/../roadmap" && pwd 2>/dev/null || echo "$SCRIPT_DIR/../roadmap")"
export ROADMAP_DIR

# Cargar librerías
source "$SCRIPT_DIR/lib/github.sh"
source "$SCRIPT_DIR/lib/labels.sh"
source "$SCRIPT_DIR/lib/milestones.sh"
source "$SCRIPT_DIR/lib/issues.sh"

# ── Comandos ─────────────────────────────────────────────────────

show_help() {
  cat <<'HELP'
sync-roadmap.sh — Sincroniza el roadmap local con GitHub Issues

USO:
  sync-roadmap.sh sync         Sincroniza labels + milestones + issues
  sync-roadmap.sh labels       Crea/actualiza labels desde labels.yaml
  sync-roadmap.sh milestones   Crea/actualiza milestones desde milestones.yaml
  sync-roadmap.sh issues       Crea/actualiza issues desde improvements.md
  sync-roadmap.sh changelog    Genera changelog desde milestones/issues cerradas
  sync-roadmap.sh check        Verifica la configuración y conexión con GitHub
  sync-roadmap.sh help         Muestra esta ayuda

VARIABLES DE ENTORNO:
  GITHUB_REPOSITORY  Repo en formato "owner/repo" (auto-detectado si no se especifica)

FLUJO DE TRABAJO:
  1. Añadir nuevas ideas a roadmap/improvements.md
  2. Ejecutar: sync-roadmap.sh sync
  3. Las issues se crean automáticamente en GitHub

  Para funcionalidades ya implementadas:
  - Añadir ítem con "state: closed" en improvements.md
  - La issue se crea y se cierra automáticamente

IDEMPOTENCIA:
  El script es completamente idempotente. Ejecutarlo múltiples veces
  no crea duplicados. Las issues se identifican por su título (que incluye el ID).
HELP
}

show_usage() {
  echo "Uso: $0 {sync|labels|milestones|issues|changelog|check|help}"
  exit 1
}

check() {
  github_check
  local repo
  repo="$(github_repo)"

  info "Repositorio: $repo"
  info "$($GH --version 2>/dev/null | head -1)"

  # Verificar archivos locales
  local all_ok=true
  for f in "$ROADMAP_DIR/labels.yaml" "$ROADMAP_DIR/milestones.yaml" "$ROADMAP_DIR/improvements.md"; do
    if [[ -f "$f" ]]; then
      ok "Encontrado: $f"
    else
      fail "Falta: $f"
      all_ok=false
    fi
  done

  # Verificar conexión GitHub
  if $GH api "/repos/$repo" --jq '.full_name' &>/dev/null; then
    ok "Conexión GitHub: OK"
  else
    fail "Conexión GitHub: ERROR — verifica que el repo existe y tienes acceso"
    all_ok=false
  fi

  if $all_ok; then
    echo ""
    info "Todo listo. Ejecuta: $0 sync"
  fi
}

changelog() {
  local repo
  repo="$(github_repo)"

  info "Generando changelog desde GitHub milestones..."

  # Obtener milestones con issues cerradas
  local milestones
  milestones="$($GH api "/repos/$repo/milestones?state=all&per_page=100" --paginate --jq '.[] | {title, description, number, state}' 2>/dev/null)"

  while IFS= read -r ms; do
    [[ -z "$ms" ]] && continue

    local ms_title ms_number ms_state
    ms_title="$(echo "$ms" | jq -r '.title // ""')"
    ms_number="$(echo "$ms" | jq -r '.number // ""')"
    ms_state="$(echo "$ms" | jq -r '.state // ""')"

    echo ""
    echo "## $ms_title"
    echo ""

    if [[ -z "$ms_number" ]]; then
      echo "(Sin issues)"
      continue
    fi

    # Obtener issues cerradas del milestone
    local issues
    issues="$($GH api "/repos/$repo/issues?milestone=$ms_number&state=closed&per_page=100" --paginate --jq '.[] | {number, title}' 2>/dev/null)"

    if [[ -z "$issues" || "$issues" == "null" ]]; then
      echo "(Sin issues cerradas)"
    else
      echo "$issues" | jq -r '"- #\(.number): \(.title)"'
    fi
  done <<< "$(echo "$milestones" | jq -c '.' 2>/dev/null || echo "")"
}

# ── Main ──────────────────────────────────────────────────────────

main() {
  github_check

  case "${1:-}" in
    sync)
      sync_labels
      sync_milestones
      sync_issues
      info "Sincronización completa."
      ;;
    labels)
      sync_labels
      ;;
    milestones)
      sync_milestones
      ;;
    issues)
      sync_issues
      ;;
    changelog)
      changelog
      ;;
    check)
      check
      ;;
    help|--help|-h)
      show_help
      ;;
    *)
      show_usage
      ;;
  esac
}

main "$@"
