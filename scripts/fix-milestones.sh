#!/bin/bash
# Fix milestone assignments for already-created issues based on improvements.md
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/lib/github.sh"

REPO="$(github_repo)"
MD_FILE="${ROADMAP_DIR:-$SCRIPT_DIR/../roadmap}/improvements.md"

# Build milestone number lookup
echo "Fetching milestones..."
declare -A MS_NUMS
while IFS= read -r line; do
  t="$(echo "$line" | jq -r '.title')"
  n="$(echo "$line" | jq -r '.number')"
  MS_NUMS["$t"]="$n"
done < <(gh api "/repos/$REPO/milestones?state=all&per_page=100" --paginate 2>/dev/null | jq -s 'add' | jq -c '.[] | {title, number}')

echo "Fetching existing closed issues..."
declare -A ISSUE_NUMS
while IFS= read -r line; do
  title="$(echo "$line" | jq -r '.title')"
  num="$(echo "$line" | jq -r '.number')"
  ms_title="$(echo "$line" | jq -r '.milestone.title // ""')"
  ISSUE_NUMS["$title"]="$num:$ms_title"
done < <(gh api "/repos/$REPO/issues?state=closed&per_page=100" --paginate 2>/dev/null | jq -s 'add' | jq -c '.[] | {number, title, milestone: {title: .milestone.title}}')

UPDATED=0
SKIPPED=0

# Parse improvements.md for closed items with milestones
in_code_block=false
while IFS= read -r line; do
  if [[ "$line" =~ ^\`{3,} ]]; then
    if $in_code_block; then in_code_block=false; else in_code_block=true; fi
    continue
  fi
  $in_code_block && continue

  # Match: - **ID**: Title `prio` `label1` `label2` `milestone`
  if [[ "$line" =~ ^[[:space:]]*-[[:space:]]*\*\*([A-Za-z0-9_.-]+)\*\*:[[:space:]]*(.+)$ ]]; then
    ID="${BASH_REMATCH[1]}"
    rest="${BASH_REMATCH[2]}"

    # Extract milestone from backtick tokens
    MS=""
    tmp="$rest"
    while [[ "$tmp" =~ \`([^\`]+)\` ]]; do
      tok="${BASH_REMATCH[1]}"
      if [[ "$tok" =~ ^v[0-9] ]]; then
        MS="$tok"
      fi
      tmp="${tmp#*"${BASH_REMATCH[0]}"}"
    done

    # Check state
    state=$(grep -A5 "^\*\*${ID}\*\*:" "$MD_FILE" 2>/dev/null | grep "state:" | head -1 | sed 's/.*state:[[:space:]]*\(open\|closed\).*/\1/')
    [[ "$state" != "closed" ]] && continue

    # Find matching GitHub issue: "ID: Title" format
    GH_TITLE="${ID}: "
    found=false
    for key in "${!ISSUE_NUMS[@]}"; do
      if [[ "$key" == "$GH_TITLE"* ]]; then
        IFS=':' read -r num current_ms <<< "${ISSUE_NUMS[$key]}"
        if [[ "$current_ms" != "$MS" && -n "$MS" ]]; then
          ms_num="${MS_NUMS[$MS]:-}"
          if [[ -n "$ms_num" ]]; then
            echo "#$num $key → milestone $MS (ms#$ms_num)"
            gh api "/repos/$REPO/issues/$num" --method PATCH \
              -f milestone="$ms_num" &>/dev/null && ((UPDATED++))
          else
            echo "WARN: Milestone not found: $MS"
          fi
        else
          ((SKIPPED++))
        fi
        found=true
        break
      fi
    done
    if ! $found; then
      echo "WARN: Issue not found for $ID (title: $GH_TITLE)"
    fi
  fi
done < "$MD_FILE"

echo "Done: $UPDATED updated, $SKIPPED skipped"
