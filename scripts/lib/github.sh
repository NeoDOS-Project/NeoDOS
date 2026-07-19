# ── GitHub API helpers ─────────────────────────────────────────────
# Usa exclusivamente gh + gh api.
# Uso: source scripts/lib/github.sh

GH="${GH:-gh}"

github_check() {
  if ! command -v "$GH" &>/dev/null; then
    echo "ERROR: gh no encontrado. Instálalo desde https://cli.github.com/" >&2
    exit 1
  fi
  if ! $GH auth status &>/dev/null; then
    echo "ERROR: gh no autenticado. Ejecuta: gh auth login" >&2
    exit 1
  fi
}

github_repo() {
  local repo="${GITHUB_REPOSITORY:-}"
  if [[ -z "$repo" ]]; then
    repo="$($GH repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true)"
  fi
  if [[ -z "$repo" ]]; then
    echo "ERROR: No se pudo determinar el repositorio." >&2
    echo "  Opción 1: export GITHUB_REPOSITORY=owner/repo" >&2
    echo "  Opción 2: gh repo view (estando dentro del repo)" >&2
    exit 1
  fi
  echo "$repo"
}

# ── API REST ──────────────────────────────────────────────────────

gh_api_get() {
  local endpoint="$1"
  $GH api "$endpoint" --jq '.' 2>/dev/null || echo "null"
}

gh_api_post() {
  local endpoint="$1"; shift
  local data="$1"
  $GH api "$endpoint" --input - <<<"$data" 2>/dev/null
}

gh_api_patch() {
  local endpoint="$1"; shift
  local data="$1"
  $GH api "$endpoint" --method PATCH --input - <<<"$data" 2>/dev/null
}

gh_api_delete() {
  local endpoint="$1"
  $GH api "$endpoint" --method DELETE --silent 2>/dev/null || true
}

# ── Paginación ────────────────────────────────────────────────────
# Maneja respuestas paginadas automáticamente

gh_api_get_all() {
  local endpoint="$1"
  local tmp
  tmp="$($GH api "$endpoint" --paginate --jq '.' 2>/dev/null)"
  if [[ -z "$tmp" || "$tmp" == "null" ]]; then
    echo "[]"
    return
  fi
  echo "$tmp"
}

# ── Colores / logging ─────────────────────────────────────────────

info()  { printf "  \033[1;34m➜\033[0m %s\n" "$*"; }
ok()    { printf "  \033[1;32m✓\033[0m %s\n" "$*"; }
warn()  { printf "  \033[1;33m⚠\033[0m %s\n" "$*"; }
fail()  { printf "  \033[1;31m✗\033[0m %s\n" "$*"; }
