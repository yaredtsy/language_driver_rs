#!/usr/bin/env bash
# The folder is the registry: print the names under languages/ (minus _template).
set -euo pipefail
cd "$(dirname "$0")/../languages"
for d in */; do d=${d%/}; [ "$d" = _template ] || echo "$d"; done
