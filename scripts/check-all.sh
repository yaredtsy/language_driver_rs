#!/usr/bin/env bash
# Same checks as CI.
#   scripts/check-all.sh --static     the cheap repo checks (seconds)
#   scripts/check-all.sh              static checks, core, and every language (slow)
#   scripts/check-all.sh <lang>       static checks, core, and one language
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

fail=0
bad() { echo "check-all: $*" >&2; fail=1; }
# print the matches and fail if there are any (the matches are the explanation)
none() { # none <what is wrong> <grep output>
  [ -z "$2" ] || { bad "$1"; echo "$2" >&2; }
}

# Code we own. Never target/ or a vendored engine.
rs() { grep -rn --include='*.rs' --exclude-dir=target --exclude-dir=vendor "$@" 2>/dev/null || true; }
rs_files() { grep -rl --include='*.rs' --exclude-dir=target --exclude-dir=vendor "$@" 2>/dev/null || true; }
crates=(languages/*/crates)

# rule 4: only sem.rs names an engine (Python's interp/ may name types sem re-exports); core names none
engine='(^|[^A-Za-z0-9_])(tsrs_|ruff_|ty_|ra_ap_)[a-z]'
none "an engine is named outside sem.rs (rule 4)" \
  "$(rs_files -E "$engine" "${crates[@]}" | grep -v '/sem\.rs$' | grep -v '^languages/python/crates/python-analysis/src/interp/' || true)"
none "core names an engine (rule 5)" "$(rs -E "$engine" core)"

# rule 1: no product words in code
none "a product word in code (rule 1)" "$(rs -iE 'vnoc|FileID|Schema/' core "${crates[@]}")"

# rule 6: no personal paths. The pattern is written in two pieces so this file does not match itself.
none "a personal path (rule 6)" \
  "$(grep -rnE --exclude-dir=target --exclude-dir=vendor "/Us""ers/|/ho""me/" core "${crates[@]}" scripts 2>/dev/null || true)"

# rule 2: a library never writes a file and never invents an ID. Allowed: *-cli (they are the program), and
# analyzer-testkit, a dev-dependency whose ANALYZER_BLESS switch writes expected files for a person to review.
none "fs::write or uuid in a library (rule 2)" \
  "$(rs -E 'fs::write|uuid' core/analyzer-core/src "${crates[@]}" | grep -v -- '-cli/' || true)"

# one lint style: every language's [workspace.lints] block equals the template's
lints() { awk '/^\[workspace\.lints/{p=1} /^\[profile/{p=0} p' "$1"; }
for f in languages/*/Cargo.toml; do
  [ "$f" = languages/_template/Cargo.toml ] && continue
  [ "$(lints "$f")" = "$(lints languages/_template/Cargo.toml)" ] || bad "$f: [workspace.lints] differs from the template's"
done

# there is no Cargo.toml at the root, on purpose
[ ! -e Cargo.toml ] || bad "a Cargo.toml at the repo root (ruff and rust-analyzer pin different salsa)"

# core stays engine-free
if (cd core && cargo tree 2>/dev/null | grep -E 'salsa|ruff_|tsrs_|ra_ap_'); then bad "core depends on an engine"; fi

# relative links in docs and the top-level markdown files resolve (anchors are not checked)
while IFS= read -r md; do
  dir=$(dirname "$md")
  while IFS= read -r link; do
    target=${link%%#*}
    [ -z "$target" ] && continue
    case "$target" in http*|mailto:*) continue ;; esac
    [ -e "$dir/$target" ] || bad "$md: broken link ($link)"
  done < <(grep -oE '\]\([^)]+\)' "$md" | sed -E 's/^\]\(//; s/\)$//; s/ .*//')
done < <(find docs languages scenarios -name '*.md' -not -path '*/target/*' -not -path '*/vendor/*'; ls ./*.md)

if [ "$fail" != 0 ]; then echo "check-all: static checks FAILED" >&2; exit 1; fi
[ "${1:-}" = "--static" ] && { echo "static checks passed"; exit 0; }

(cd core && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test)

langs=${1:-$(scripts/list-languages.sh)}
for l in $langs; do
  (cd "languages/$l" && cargo fmt --check &&
    cargo clippy -p "$l-syntax" -p "$l-analysis" -p "$l-cli" --all-targets -- -D warnings &&
    cargo test)
done
