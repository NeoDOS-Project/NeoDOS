#!/usr/bin/env bash
# Rule 3.5.1 guard (INV-3, docs/architecture/source-of-truth.md).
#
# No interrupt handler may block on a lock that interruptible (IF=1) code on the
# same CPU can hold. A lock that is taken from an IRQ handler must therefore be
# IRQ-safe at *every* site: either every acquisition goes through an IRQ-safe
# accessor (`without_interrupts` / `IrqMutex`), or the handler uses `try_lock()`.
#
# This guard enforces that invariant for the locks that are known to be
# IRQ-reachable: a blocking `lock()` on them is only allowed in the files that
# hold a sanctioned IRQ-safe wrapper (and in test modules, which take locks on
# purpose). Anywhere else it is a Rule 3.5.1 violation — the class of bug fixed
# in #675 (graphics RENDERER), #676 (MSI_HANDLER_TABLE) and #677
# (IRP_POOL.inner). Comment lines are ignored.
#
# Add a rule whenever a new lock becomes reachable from an IRQ handler.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/neodos-kernel/src"

if [ ! -d "$SRC" ]; then
  echo "check-irq-locks: ERROR: kernel source not found at $SRC" >&2
  exit 2
fi

# name | blocking-acquire regex | files allowed to contain it (relative to src)
RULES=(
  'graphics::RENDERER (timer cursor blink / keyboard Alt+F1-F8 VT switch)|RENDERER\.lock\(\)|graphics/mod.rs arch/x64/paging.rs'
  'idt::MSI_HANDLER_TABLE (msi_dispatch runs in IRQ context)|MSI_HANDLER_TABLE\.lock\(\)|arch/x64/idt/mod.rs'
  'irp::IRP_POOL.inner (reachable from the timer IRQ DPC path)|IRP_POOL\.inner\.lock\(\)|irp/mod.rs'
)

rc=0
count=0

is_allowed() { # <relpath> <space-separated allowed list>
  local rel="$1" allowed="$2" a
  # Regression tests deliberately hold these locks.
  case "$rel" in
    */tests/*|*/tests.rs|tests/*|*_tests.rs) return 0 ;;
  esac
  for a in $allowed; do
    [ "$rel" = "$a" ] && return 0
  done
  return 1
}

while IFS='|' read -r name pat allowed; do
  [ -z "${name:-}" ] && continue
  count=$((count + 1))
  while IFS= read -r hit; do
    [ -z "$hit" ] && continue
    rel="${hit%%:*}"          # path:line:content -> path
    rel="${rel#"$SRC"/}"
    code="${hit#*:*:}"        # content after the 2nd colon
    # Strip line comments so a mention inside `// ...` is not a violation.
    code="${code%%//*}"
    if ! printf '%s\n' "$code" | grep -Eq "$pat"; then
      continue
    fi
    if ! is_allowed "$rel" "$allowed"; then
      echo "IRQ-LOCK VIOLATION [Rule 3.5.1]: $name"
      echo "  blocking acquire outside its IRQ-safe wrapper: $rel"
      echo "    $hit"
      rc=1
    fi
  done < <(grep -rn --include='*.rs' -E "$pat" "$SRC" || true)
done < <(printf '%s\n' "${RULES[@]}")

if [ "$rc" -eq 0 ]; then
  echo "check-irq-locks: OK ($count IRQ-reachable lock rule(s))"
else
  echo "check-irq-locks: FAILED"
  echo "  Make the lock IRQ-safe at every site (without_interrupts / IrqMutex),"
  echo "  or use try_lock() in the handler. See docs/architecture/source-of-truth.md."
fi
exit "$rc"
