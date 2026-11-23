#!/usr/bin/env bash
# Try the TypeScript analyzer on any project. Only reads the project.
#
#   scripts/ts-try.sh calls <root> <file> [entry] [more typescript-cli flags]
#       the call tree of one def (entry = its qualname, e.g. `Panel.render`), or of the file's own code when no entry is given
#       <root>  the project folder (the config is searched from <file> up to it)
#       <file>  a file of the project, relative to <root>
#       more flags: --graph, --wire, --timing, ...   (typescript-cli calls --help lists them)
#
#   scripts/ts-try.sh sweep <root> <file> [depth] [only] [entry]
#       a graph for EVERY def of the project: counts, time, memory, the slowest entries; fails on decl_not_in_outline / engine_kind_unhandled
#       [depth]  default 1;  [only]  keep files whose path contains this text;  [entry]  keep defs with this qualname
#
# Examples:
#   scripts/ts-try.sh calls ~/work/app src/App.tsx App
#   scripts/ts-try.sh calls ~/work/app src/App.tsx "" --timing
#   scripts/ts-try.sh sweep ~/work/app src/App.tsx
#   scripts/ts-try.sh sweep ~/vscode/src vs/base/common/event.ts 1 vs/base/common
set -euo pipefail

usage() { sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
[ $# -ge 3 ] || usage

mode=$1 root=$2 file=$3
shift 3
[ -d "$root" ] || { echo "ts-try: $root is not a folder" >&2; exit 2; }
root="$(cd "$root" && pwd)"
[ -f "$root/$file" ] || { echo "ts-try: $root/$file is not a file" >&2; exit 2; }

export PATH="$HOME/.cargo/bin:$PATH"
cd "$(cd "$(dirname "$0")/.." && pwd)/languages/typescript"

case "$mode" in
  calls)
    entry=${1:-}
    [ $# -eq 0 ] || shift
    args=(calls "$root" "$file")
    [ -z "$entry" ] || args+=(--entry "$entry")
    exec cargo run -q -p typescript-cli -- "${args[@]}" "$@"
    ;;
  sweep)
    export TS_SWEEP_ROOT="$root" TS_SWEEP_FILE="$file" TS_SWEEP_DEPTH="${1:-1}"
    [ -z "${2:-}" ] || export TS_SWEEP_ONLY="$2"
    [ -z "${3:-}" ] || export TS_SWEEP_ENTRY="$3"
    exec cargo test -p typescript-analysis --test sweep -- --ignored --nocapture
    ;;
  *) usage ;;
esac
