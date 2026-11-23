#!/usr/bin/env bash
# The sweep (docs/08-bend/03 §5 step 5): every def of the book of one entry file entered once, and what that came to.
#   scripts/bend-sweep.sh <root> <entry file, relative to <root>>
# Builds bend-cli in release, then runs `bend-cli calls <root> <entry> --all` under `/usr/bin/time -l` (macOS; `-v` on Linux) and prints its table (files, defs,
# entered, contexts, edges by kind, unresolved by reason, every counter summed, the slowest request) with the wall time and the peak memory (maximum resident
# set size and, on macOS, the peak memory footprint). Reads the project; writes nothing but cargo's target/. NO_BUILD=1 uses the bend-cli already built.
#   scripts/bend-sweep.sh ../language_clone/bend/demos/app_slash_boss_3d PROOF.bend
set -euo pipefail
[ $# -eq 2 ] || { sed -n '2,7p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
root="$1" entry="$2"
[ -d "$root" ] || { echo "bend-sweep: $root is not a folder" >&2; exit 2; }
root="$(cd "$root" && pwd)"
[ -f "$root/$entry" ] || { echo "bend-sweep: $root/$entry is not a file" >&2; exit 2; }

repo="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
cd "$repo/languages/bend"
[ -n "${NO_BUILD:-}" ] || cargo build -q --release -p bend-cli
cli="${CARGO_TARGET_DIR:-$repo/languages/bend/target}/release/bend-cli"

measured="$(mktemp)"
trap 'rm -f "$measured"' EXIT
if [ "$(uname)" = Darwin ]; then
  /usr/bin/time -l "$cli" calls "$root" "$entry" --all 2> "$measured"
else
  /usr/bin/time -v "$cli" calls "$root" "$entry" --all 2> "$measured"
fi
if [ "$(uname)" = Darwin ]; then
  awk '/ real /{print "wall: " $1 " s"} /maximum resident set size/{printf "peak resident: %.1f MB\n", $1/1048576} /peak memory footprint/{printf "peak footprint: %.1f MB\n", $1/1048576}' "$measured"
else
  awk '/Elapsed \(wall clock\)/{print "wall: " $NF} /Maximum resident set size/{printf "peak resident: %.1f MB\n", $NF/1024}' "$measured"
fi
