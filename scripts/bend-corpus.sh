#!/usr/bin/env bash
# S-BD1 (docs/08-bend/02-phase-1-parsing.md §1): which files of the Bend corpus does Bend itself accept?
#   scripts/bend-corpus.sh <path to the bend clone> [out dir]
# Corpus: every .bend under tests/, demos/, bench/, plus bend2/base.bend. Writes to <out dir> (default languages/bend/spikes/out, ignored by git):
#   files.txt             the corpus, paths relative to the clone, sorted
#   check-accepted.txt    `<file> --check-only` passes (parse, name resolution, type check)
#   check-refused.txt     it fails (exit 1), or does not finish in 120 s (listed in check-other.txt)
#   parse-accepted.txt    Bend's loader (book_load: imports, parse_book, name resolution; no checker) accepts. THE LIST A PARSER MUST ACCEPT:
#                         a file that only fails to type-check still parses
#   parse-refused.txt     <file> TAB <UTF-16 offset or -> TAB <what Bend expected>  (also refused: forms the loader rejects after parsing, e.g. a match on a non-parameter)
#   parse-decls.tsv       for each parse-accepted file, the names Bend sees, in order: <file> TAB type|law|def|ctr TAB <name> TAB <parameters or fields> TAB <templates> (a def that fills a law is `def`; a law of another file that it fills is named `<namespace>:<name>`)
#   declared-negative.txt files whose first `#|` line says SOME PROOFS FAIL or Error: (gates/test.ts FAILS); the cross-check and the fallback
# The oracle is, in order: `bun <clone>/bend2/main.ts` (the pinned clone: the grammar's source of truth), then an installed `bend` (warns:
# it may be newer than the pin), then the `#|` convention (no check or parse lists, only declared-negative.txt). Nothing is fetched, nothing
# under ~/.bend is read or written: telemetry is off, the hub is a closed port, the package folder is under <out dir>.
set -euo pipefail
export LC_ALL=C
root="$(cd "$(dirname "$0")/.." && pwd)"
[ $# -ge 1 ] || { echo "usage: scripts/bend-corpus.sh <path to the bend clone> [out dir]" >&2; exit 2; }
clone="$(cd "$1" && pwd)"
[ -f "$clone/bend2/bend.ts" ] || { echo "$clone is not a bend clone (no bend2/bend.ts)" >&2; exit 2; }
out="${2:-$root/languages/bend/spikes/out}"
mkdir -p "$out/lib" "$out/work"
out="$(cd "$out" && pwd)"
jobs=3 # an 8 GB machine: a few bun processes, not one per core

export BEND_NO_TELEMETRY=1 BEND_LIB="$out/lib" BEND_HUB="http://127.0.0.1:9"
export CORPUS_CLONE="$clone" CORPUS_OUT="$out"
[ -d "$HOME/.bun/bin" ] && PATH="$HOME/.bun/bin:$PATH"

echo "grammar pin: $(git -C "$clone" log -1 --format=%h 2>/dev/null || echo unknown)  (clone: $clone)"
(cd "$clone" && { find tests demos bench -name '*.bend'; echo bend2/base.bend; }) | LC_ALL=C sort > "$out/files.txt"
total=$(wc -l < "$out/files.txt" | tr -d ' ')
echo "corpus: $total files"

# declared-negative: the first `#|` line of a test is the head of the output it expects
: > "$out/declared-negative.txt"
while IFS= read -r f; do
  first=$(grep -m1 '^#|' "$clone/$f" || true)
  case "$first" in '#|SOME PROOFS FAIL'*|'#|Error:'*) echo "$f" >> "$out/declared-negative.txt" ;; esac
done < "$out/files.txt"
echo "declared negative by their own #| lines: $(wc -l < "$out/declared-negative.txt" | tr -d ' ')"

# [check] 3 of docs/08-bend/05: what is the first line of a file that is not blank?
nocomment=0; nodecl=0
while IFS= read -r f; do
  first=$(grep -m1 -v '^[[:space:]]*$' "$clone/$f" || true)
  case "$first" in '#'*) ;; *) nocomment=$((nocomment + 1)) ;; esac
  code=$(grep -m1 -v -e '^[[:space:]]*$' -e '^[[:space:]]*#' "$clone/$f" || true)
  case "$code" in import*|def\ *|type\ *|law\ *|@unsafe*) ;; *) nodecl=$((nodecl + 1)) ;; esac
done < "$out/files.txt"
echo "files whose first non-blank line is not a comment: $nocomment; whose first code line is not import/def/type/law/@unsafe: $nodecl"
echo "files with a hub import (0x<hash>/ or name@version/): $(cd "$clone" && xargs grep -lE '^import[[:space:]]+(0x[0-9a-f]+/|[a-z][a-z0-9-]*@[0-9])' < "$out/files.txt" 2>/dev/null | wc -l | tr -d ' ')"

oracle=""
if [ -f "$clone/bend2/main.ts" ] && command -v bun >/dev/null; then
  oracle=bun
elif command -v bend >/dev/null; then
  oracle=bend
  echo "WARNING: using the installed bend ($(bend version 2>/dev/null || echo '?')); it may be newer than the grammar pin" >&2
fi

if [ -z "$oracle" ]; then
  echo "NO ORACLE: neither bun (with $clone/bend2/main.ts) nor bend is on PATH." >&2
  echo "FALLBACK (02 §1): a file whose first #| line is an error is a negative, every other file a positive. No check or parse lists written." >&2
  exit 0
fi
echo "oracle: $oracle"

# 1. `--check-only` on every file, $jobs at a time, each with a 120 s alarm; one `<exit code> TAB <file>` line each (PARSE_ONLY=1 keeps the
# lists of an earlier run: the check takes minutes, the loader seconds)
export CORPUS_ORACLE="$oracle"
if [ -z "${PARSE_ONLY:-}" ]; then
tr '\n' '\0' < "$out/files.txt" | xargs -0 -n1 -P"$jobs" bash -c '
  f=$1
  cd "$CORPUS_OUT/work"
  if [ "$CORPUS_ORACLE" = bun ]; then cmd=(bun "$CORPUS_CLONE/bend2/main.ts"); else cmd=(bend); fi
  perl -e "alarm 120; exec @ARGV" "${cmd[@]}" "$CORPUS_CLONE/$f" --check-only > /dev/null 2>&1 && code=0 || code=$?
  printf "%s\t%s\n" "$code" "$f"
' _ | LC_ALL=C sort -t$'\t' -k2 > "$out/check-results.tsv"
fi
awk -F'\t' '$1 == 0 { print $2 }' "$out/check-results.tsv" > "$out/check-accepted.txt"
awk -F'\t' '$1 != 0 { print $2 }' "$out/check-results.tsv" > "$out/check-refused.txt"
awk -F'\t' '$1 != 0 && $1 != 1 { print $1 "\t" $2 }' "$out/check-results.tsv" > "$out/check-other.txt"
echo "--check-only accepted: $(wc -l < "$out/check-accepted.txt" | tr -d ' ')   refused: $(wc -l < "$out/check-refused.txt" | tr -d ' ')   (neither accept nor clean refusal: $(wc -l < "$out/check-other.txt" | tr -d ' '))"

# 2. the loader alone, in one process: parse_book and name resolution, no checker (bend2/main.ts book_read minus book_valid)
cat > "$out/parse-oracle.ts" <<'EOF'
import * as fs from "node:fs";
const clone = process.env.CORPUS_CLONE!;
const out = process.env.CORPUS_OUT!;
const Bend = await import(clone + "/bend2/bend.ts");
const files = fs.readFileSync(out + "/files.txt", "utf8").split("\n").filter((l) => l !== "");
const ok: string[] = [];
const bad: string[] = [];
const decls: string[] = [];
const expected = (e: any): string => {
  const s = typeof e === "string" ? e : typeof e?.exp === "string" ? e.exp : JSON.stringify(e?.exp ?? String(e));
  return s.split("\n")[0].slice(0, 160);
};
for (const f of files) {
  try {
    const book = Bend.book_nil();
    const n0 = await Bend.book_load(book, clone + "/" + f, "", new Map());
    ok.push(f);
    const seen = new Set<string>();
    for (const k of book.order.slice(n0)) {
      const tld = book.tlds[k];
      const first = !seen.has(k);
      seen.add(k);
      // a name with a `:` is `<namespace>:<name>`, a law of another file that this file's def fills (`def Laws.x(..)` through an alias)
      const kind = tld.$ === "ADT" ? "type" : first && tld.l === true && !k.includes(":") ? "law" : "def";
      // columns 4 and 5: the parameters (a type's, a def's, a constructor's fields) and, for a def, how many of them are templates; `-` for a law
      decls.push(f + "\t" + kind + "\t" + k + "\t" + (kind === "law" ? "-" : tld.n) + "\t" + (kind === "def" ? tld.x : "-"));
      if (tld.$ === "ADT") {
        for (const c of tld.c) {
          decls.push(f + "\tctr\t" + c.k + "\t" + c.n + "\t-");
        }
      }
    }
  } catch (e: any) {
    bad.push(f + "\t" + (e?.spn?.beg ?? "-") + "\t" + expected(e));
  }
}
fs.writeFileSync(out + "/parse-accepted.txt", ok.map((l) => l + "\n").join(""));
fs.writeFileSync(out + "/parse-refused.txt", bad.map((l) => l + "\n").join(""));
fs.writeFileSync(out + "/parse-decls.tsv", decls.map((l) => l + "\n").join(""));
EOF
(cd "$out/work" && bun "$out/parse-oracle.ts")
echo "loader accepted (parse + names, no checker): $(wc -l < "$out/parse-accepted.txt" | tr -d ' ')   refused: $(wc -l < "$out/parse-refused.txt" | tr -d ' ')"

# the numbers that tell whether the lists agree
echo "check-accepted but not parse-accepted (must be 0): $(comm -23 "$out/check-accepted.txt" "$out/parse-accepted.txt" | wc -l | tr -d ' ')"
echo "declared negative but check-accepted (a test whose #| is stale?): $(comm -12 "$out/declared-negative.txt" "$out/check-accepted.txt" | wc -l | tr -d ' ')"
echo "not declared negative but check-refused: $(comm -13 "$out/declared-negative.txt" "$out/check-refused.txt" | wc -l | tr -d ' ')"
echo "lists are in $out"
