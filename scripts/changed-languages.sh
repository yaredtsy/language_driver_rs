#!/usr/bin/env bash
# usage: scripts/changed-languages.sh <base-sha> [<head>]
# Prints two lines for $GITHUB_OUTPUT:  core=true|false  and  languages=["a","b"]
# A change under core/ or to the shared toolchain, lint, script or CI files means every language. A change under
# languages/<lang>/ means that language. No usable base (a new branch) means everything.
set -euo pipefail
cd "$(dirname "$0")/.."
base=${1:-}
head=${2:-HEAD}
all=$(scripts/list-languages.sh)

if [ -z "$base" ] || [ "$base" = 0000000000000000000000000000000000000000 ] || ! git cat-file -e "$base^{commit}" 2>/dev/null; then
  files=$'core/\nscripts/'
else
  files=$(git diff --name-only "$base" "$head")
fi

core=false
langs=""
everything=false
while IFS= read -r f; do
  case "$f" in
    core/*|scripts/*|.github/*|rust-toolchain.toml|rustfmt.toml|clippy.toml|.gitattributes) core=true; everything=true ;;
    scenarios/*) everything=true ;;
    languages/_template/*) ;;
    languages/*/*) l=${f#languages/}; langs="$langs ${l%%/*}" ;;
  esac
done <<< "$files"

if [ "$everything" = true ]; then langs=$all; fi
json="["
sep=""
for l in $(printf '%s\n' $langs | sort -u); do
  # only names that exist, in case a language folder was deleted
  grep -qx "$l" <<< "$all" || continue
  json="$json$sep\"$l\""; sep=","
done
echo "core=$core"
echo "languages=$json]"
