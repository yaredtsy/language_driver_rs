#!/usr/bin/env bash
# Same checks as CI. --static runs only the cheap repo checks.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
# core must not depend on an engine
if (cd core && cargo tree 2>/dev/null | grep -E 'salsa|ruff_|tsrs_|ra_ap_'); then
  echo "core depends on an engine" >&2; exit 1
fi
[ "${1:-}" = "--static" ] && exit 0
(cd core && cargo fmt --check && cargo clippy -- -D warnings && cargo test)
for l in $(scripts/list-languages.sh); do (cd "languages/$l" && cargo fmt --check && cargo test); done
