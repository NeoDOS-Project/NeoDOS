#!/usr/bin/env bash
# Regenerate data/keyboard/<tag>.kbd from the XKB layouts with xkbcompile.
#
# `layout:variant:tag` — the tag is the NeoDOS layout name/lang (locale tag).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

(cd tools/xkbcompile && cargo build --quiet)
bin=tools/xkbcompile/target/debug/xkbcompile

# Keep in sync with check-keyboard-layouts.sh.
layouts=(
    "us:basic:en-US"
    "es:deadtilde:es-ES"
    "es:cat:ca-ES"
    "fr:basic:fr-FR"
    "de:basic:de-DE"
    "it:basic:it-IT"
    "pt:basic:pt-PT"
    "br:abnt2:pt-BR"
    "nl:basic:nl-NL"
    "se:basic:sv-SE"
    "dk:basic:da-DK"
    "no:basic:nb-NO"
    "fi:kotoistus:fi-FI"
    "pl:basic:pl-PL"
    "cz:basic:cs-CZ"
)

for spec in "${layouts[@]}"; do
    IFS=: read -r layout variant tag <<<"$spec"
    "$bin" "$layout" "$variant" --name "$tag" --lang "$tag" --out "data/keyboard/$tag.kbd"
done

echo "gen-keyboard-layouts: done"
