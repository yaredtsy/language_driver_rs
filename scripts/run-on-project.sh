#!/usr/bin/env bash
# scripts/run-on-project.sh <lang> <dir> <file> <entry> [flags passed on to `calls`]
# For a person, not for CI: build <lang>-cli, run `calls <dir> <file> --entry <entry> … --wire` and the same with `--merged` on a real project,
# and write out/run/wire.json and out/run/wire-merged.json (out/ is not in git). The prefixes of the ids are flags passed on, never written here:
#   scripts/run-on-project.sh python <dir> <file> <entry> --function-prefix <p> --class-prefix <p>
# With GOLDEN=<folder> each file is compared with the file of the same name there, AS JSON (the keys of an object may be in any order, the
# arrays are compared in order): `wire.json: equal` or `wire.json: differs`, and exit 1 on a difference.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
lang="${1:?usage: run-on-project.sh <lang> <dir> <file> <entry> [flags passed on to calls]}"
dir="${2:?usage: run-on-project.sh <lang> <dir> <file> <entry> [flags passed on to calls]}"
file="${3:?usage: run-on-project.sh <lang> <dir> <file> <entry> [flags passed on to calls]}"
entry="${4:?usage: run-on-project.sh <lang> <dir> <file> <entry> [flags passed on to calls]}"
shift 4

# Paths the caller gave are relative to where the caller is: make them absolute before the cd below.
dir="$(cd "$dir" && pwd)"
golden="${GOLDEN:-}"
[ -z "$golden" ] || golden="$(cd "$golden" && pwd)"

cd "$root/languages/$lang"
cargo build -q -p "$lang-cli"
cli="${CARGO_TARGET_DIR:-$root/languages/$lang/target}/debug/$lang-cli"

out="$root/out/run"
mkdir -p "$out"
"$cli" calls "$dir" "$file" --entry "$entry" "$@" --wire > "$out/wire.json"
"$cli" calls "$dir" "$file" --entry "$entry" "$@" --wire --merged > "$out/wire-merged.json"
echo "wrote $out/wire.json and $out/wire-merged.json"

[ -n "$golden" ] || exit 0
differs=0
for name in wire.json wire-merged.json; do
  if [ ! -f "$golden/$name" ]; then
    echo "$name: no such file in the golden folder"
    differs=1
  elif python3 -I -c 'import json, sys; sys.exit(0 if json.load(open(sys.argv[1])) == json.load(open(sys.argv[2])) else 1)' "$out/$name" "$golden/$name"; then
    echo "$name: equal"
  else
    echo "$name: differs"
    differs=1
  fi
done
exit "$differs"
