#!/usr/bin/env bash
# scripts/check-proofs.sh <lang>
# A language's compile-fail proofs are doctests behind the feature `__proof` of `<lang>-analysis`. With the feature off rustdoc finds no
# doctest in them and reports success, so this runs them with it on and checks that exactly the proofs listed in
# languages/<lang>/crates/<lang>-analysis/proof-doctests.txt ran and passed. A language without that file has no proofs.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
lang="${1:?usage: check-proofs.sh <lang>}"
crate="$root/languages/$lang/crates/$lang-analysis"
expected="$crate/proof-doctests.txt"
[ -f "$expected" ] || exit 0

want_fail=$(sed -n 's/^compile_fail=//p' "$expected")
want_pass=$(sed -n 's/^pass=//p' "$expected")
cd "$root/languages/$lang"

cargo clippy -p "$lang-analysis" --all-targets --features __proof -- -D warnings
out=$(cargo test -p "$lang-analysis" --doc --features __proof 2>&1) || { echo "$out" >&2; echo "check-proofs: a proof doctest failed" >&2; exit 1; }

# a proof doctest is named "<file> - proof (line N)", with " - compile fail" for the ones that must not compile
got_fail=$(grep -cE ' - proof \(line [0-9]+\) - compile fail \.\.\. ok$' <<<"$out" || true)
got_pass=$(grep -cE ' - proof \(line [0-9]+\) \.\.\. ok$' <<<"$out" || true)
if [ "$got_fail" != "$want_fail" ] || [ "$got_pass" != "$want_pass" ]; then
  echo "$out" >&2
  echo "check-proofs: expected $want_fail compile-fail and $want_pass compiling proof doctests, ran $got_fail and $got_pass" >&2
  exit 1
fi
echo "check-proofs: $lang: $got_fail compile-fail and $got_pass compiling proof doctests passed"
