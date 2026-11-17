#!/usr/bin/env bash
# check-skills.sh — keep skills/*/SKILL.md from drifting away from docs/code (#605).
#
# Skills are procedural checklists executed by AI agents, so stale guidance
# (removed scripts, obsolete heuristics, old paths/macros) turns directly into
# wrong actions. This is a lightweight guard with two checks:
#
#   A. Every `scripts/<name>` referenced by a skill must exist on disk.
#      Self-maintaining: deleting a script breaks any skill that still cites it.
#   B. A small denylist of known-obsolete tokens (old heuristics, stale paths,
#      removed macros / test harness names) must not appear — except on lines
#      that deliberately document them as obsolete (per-file allowlist).
#
# Run from anywhere. Exit 0 = skills in sync, 1 = drift detected.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

shopt -s nullglob globstar
files=(skills/**/SKILL.md)
if [ "${#files[@]}" -eq 0 ]; then
    echo "check-skills: no skills/**/SKILL.md found" >&2
    exit 1
fi

fail=0

# --- A. referenced scripts must exist ---------------------------------------
# Any `scripts/foo.sh` mentioned by a skill has to be a real file.
while IFS= read -r ref; do
    [ -n "$ref" ] || continue
    if [ ! -e "$ref" ]; then
        echo "check-skills: stale script reference '$ref' (file not found)" >&2
        grep -HnF -- "$ref" "${files[@]}" | sed 's/^/    /' >&2 || true
        fail=1
    fi
done < <(grep -hoE 'scripts/[A-Za-z0-9_.-]+' "${files[@]}" | sort -u)

# --- B. obsolete tokens ------------------------------------------------------
# Each rule is:  <reason> <TAB> <deny-regex>
# A finding is suppressed when it matches an allow rule:
#                <file-glob> <TAB> <line-regex>
# Extend these lists when a refactor makes something else obsolete — that is the
# whole point: the guard is only as good as the drift it has been taught.
denylist=(
    $'obsolete "RAX >= 77" heuristic — the rule is Ob-based, not numeric\tRAX[[:space:]]*>=[[:space:]]*77'
    $'obsolete Registry range RAX 67-76 — it is RAX 50-59\tRAX[[:space:]]*67[[:space:]]*(-|–)[[:space:]]*76'
    $'obsolete TestSpec/TestResult harness — use test_case!\t(TestSpec|TestResult)'
    $'removed nem_driver! macro — NEM v3 drivers are standalone libs\tnem_driver!'
    $'stale path src/vfs/\tsrc/vfs/'
    $'stale path src/slab.rs\tsrc/slab\\.rs'
    $'stale path src/handle.rs\tsrc/handle\\.rs'
    $'stale path src/work_queue.rs\tsrc/work_queue\\.rs'
)

allowlist=(
    $'skills/syscalls/SKILL.md\tRAX[[:space:]]*>=[[:space:]]*77'
    $'skills/review/SKILL.md\tRAX[[:space:]]*>=[[:space:]]*77'
    $'skills/drivers/SKILL.md\tnem_driver!'
    $'skills/ipc/SKILL.md\tsrc/handle\\.rs'
    $'skills/ipc/SKILL.md\tsrc/work_queue\\.rs'
)

is_allowed() {
    local path="$1" text="$2" rule f re
    for rule in "${allowlist[@]}"; do
        IFS=$'\t' read -r f re <<<"$rule"
        if [[ "$path" == $f && "$text" =~ $re ]]; then
            return 0
        fi
    done
    return 1
}

for rule in "${denylist[@]}"; do
    IFS=$'\t' read -r reason deny <<<"$rule"
    while IFS=: read -r path lineno text; do
        [ -n "$path" ] || continue
        is_allowed "$path" "$text" && continue
        printf 'check-skills: %s:%s: %s\n' "$path" "$lineno" "$reason" >&2
        printf '    %s\n' "$text" >&2
        fail=1
    done < <(grep -HnE -- "$deny" "${files[@]}" || true)
done

if [ "$fail" -ne 0 ]; then
    echo "check-skills: FAILED — skills drifted from docs/code (#605)" >&2
    exit 1
fi
echo "check-skills: OK (${#files[@]} skills)"
