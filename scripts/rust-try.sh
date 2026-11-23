#!/usr/bin/env bash
# Try the Rust analyzer on any project. Only reads the project (a project with no Cargo.lock is loaded with the lock written to a temporary folder).
#
#   scripts/rust-try.sh outline <root> <file>
#       the defs of one file, as qualnames: copy one into `calls`. A second def of one name is printed `name #1`.
#
#   scripts/rust-try.sh calls <root> <file> <entry> [more rust-cli flags]
#       the call tree under one def, then the breakdown (warnings, edges by kind, what is unresolved or open)
#       <root>   the project folder (the Cargo.toml is searched from <file> up to it)
#       <file>   a file of the project, relative to <root>
#       <entry>  the def's qualname as `outline` prints it (`Rec::get`, `"Rec::get #1"` or `Rec::get#1`, `tests::a_test`, `main`)
#       more flags: --timing, --graph, --wire [--merged], --depth N, --max-contexts N, --max-nodes N   (rust-cli calls --help lists them)
#
# NO_BUILD=1 scripts/rust-try.sh ...   skips the `cargo build` and uses the rust-cli already built.
#
# Examples:
#   scripts/rust-try.sh outline ~/work/app src/lib.rs
#   scripts/rust-try.sh calls ~/work/app src/lib.rs run
#   scripts/rust-try.sh calls ~/work/app src/net/client.rs Client::send --timing --depth 3
set -euo pipefail

self="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
usage() { sed -n '2,19p' "$self" | sed 's/^# \{0,1\}//'; exit 2; }
[ $# -ge 3 ] || usage

mode=$1 root=$2 file=$3
shift 3
[ -d "$root" ] || { echo "rust-try: $root is not a folder" >&2; exit 2; }
root="$(cd "$root" && pwd)"
file="${file#"$root"/}"   # a path under the root may be given in full
file="${file#/}"
[ -f "$root/$file" ] || { echo "rust-try: $root/$file is not a file" >&2; exit 2; }

export PATH="$HOME/.cargo/bin:$PATH"
cd "$(cd "$(dirname "$0")/.." && pwd)/languages/rust"

# Not `cargo run -q`: another cargo in this repo holds the build lock for minutes (it compiles the engine), and `-q` hides "Blocking waiting for file lock".
# NO_BUILD=1 uses the rust-cli already built (it may be older than the source), for when that wait is not wanted.
if [ -z "${NO_BUILD:-}" ]; then
  echo "rust-try: building rust-cli (the first build, or a build another cargo is running, takes minutes; NO_BUILD=1 skips it)..." >&2
  cargo build -p rust-cli 2>&1 | grep -v "^ *\(Compiling\|Fresh\|Downloaded\)" >&2 || true
fi
cli="${CARGO_TARGET_DIR:-$PWD/target}/debug/rust-cli"
[ -x "$cli" ] || { echo "rust-try: no $cli: the build failed" >&2; exit 1; }

case "$mode" in
  outline)
    exec "$cli" outline "$root/$file"
    ;;
  calls)
    [ $# -ge 1 ] || usage
    entry=$1
    shift
    args=(calls "$root" "$file")
    # `Rec::get #1` (as `outline` prints it) or `Rec::get#1` is the second `Rec::get` of the file
    if [[ "$entry" =~ ^(.*[^\ ])\ ?#([0-9]+)$ ]]; then
      args+=(--entry "${BASH_REMATCH[1]}" --occurrence "${BASH_REMATCH[2]}")
    else
      args+=(--entry "$entry")
    fi
    exec "$cli" "${args[@]}" "$@"
    ;;
  *) usage ;;
esac
