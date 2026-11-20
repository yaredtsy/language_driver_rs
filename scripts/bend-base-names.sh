#!/usr/bin/env bash
# S-BD3 (docs/08-bend/02-phase-1-parsing.md §1): the top-level names of Bend's Base, found with a regex now, with the parser once it exists.
#   scripts/bend-base-names.sh <path to the bend clone>
# Prints `<kind> <name>`, kind one of def, type, law, ctr, sorted bytewise: `^(@unsafe )?def NAME`, `^type NAME`, `^law NAME`, and the
# constructor lines (`  NAME{`) indented under a type. Nothing is written: BD3 turns this into a generated list once Q-BD3 is answered.
set -euo pipefail
export LC_ALL=C
[ $# -eq 1 ] || { echo "usage: scripts/bend-base-names.sh <path to the bend clone>" >&2; exit 2; }
base="$1/bend2/base.bend"
[ -f "$base" ] || { echo "no $base" >&2; exit 2; }
# a name is letters, digits, `_` and `.` (bend.ts char_is_name); a def's `?` and `(` end it
awk '
  /^[^ \t#]/ { in_type = 0 }
  match($0, /^(@unsafe )?def [A-Za-z_][A-Za-z0-9_.]*/) { n = substr($0, RSTART, RLENGTH); sub(/^(@unsafe )?def /, "", n); print "def " n; next }
  match($0, /^type [A-Za-z_][A-Za-z0-9_.]*/)           { n = substr($0, RSTART + 5, RLENGTH - 5); print "type " n; in_type = 1; next }
  match($0, /^law [A-Za-z_][A-Za-z0-9_.]*/)            { n = substr($0, RSTART + 4, RLENGTH - 4); print "law " n; next }
  in_type && match($0, /^[ \t]+[A-Za-z_][A-Za-z0-9_.]*\{/) { n = substr($0, RSTART, RLENGTH - 1); gsub(/[ \t]/, "", n); print "ctr " n }
' "$base" | sort
