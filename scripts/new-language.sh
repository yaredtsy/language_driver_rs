#!/usr/bin/env bash
# usage: scripts/new-language.sh <name>   (lowercase, e.g. go)
set -euo pipefail
name=${1:?usage: new-language.sh <name>}
root="$(cd "$(dirname "$0")/.." && pwd)"
dest="$root/languages/$name"
[ -e "$dest" ] && { echo "languages/$name exists" >&2; exit 1; }
cp -R "$root/languages/_template" "$dest"
# rename crate folders, then replace the placeholder inside text files
for d in "$dest"/crates/__lang__-*; do mv "$d" "${d/__lang__/$name}"; done
find "$dest" -type f -exec sed -i.bak "s/__lang__/$name/g" {} + && find "$dest" -name '*.bak' -delete
echo "created languages/$name"
