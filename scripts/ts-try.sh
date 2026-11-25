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
#   scripts/ts-try.sh cost <root> <file> [sample] [rounds]
#       what the context layer costs (D62, a measure, not a gate): a release sweep with the layer off and on, alternating, `rounds` times each;
#       prints the median wall time and peak memory of each and the two ratios on / off; exits 0 unless a run fails
#       [sample]  a file of sample lines (`<file>` TAB `<qualname>`), its entries whole; without it, every def and file at depth 1
#       [rounds]  default 3
#
# Examples:
#   scripts/ts-try.sh calls ~/work/app src/App.tsx App
#   scripts/ts-try.sh calls ~/work/app src/App.tsx "" --timing
#   scripts/ts-try.sh sweep ~/work/app src/App.tsx
#   scripts/ts-try.sh sweep ~/vscode/src vs/base/common/event.ts 1 vs/base/common
#   scripts/ts-try.sh cost ~/work/app src/App.tsx "" 3
set -euo pipefail

usage() { sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
[ $# -ge 3 ] || usage

start_dir=$PWD
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
  cost)
    sample=${1:-}
    rounds=${2:-3}
    case "$rounds" in
      '' | *[!0-9]* | 0) echo "ts-try: rounds must be a positive whole number, not '$rounds'" >&2; exit 2 ;;
    esac
    if [ -n "$sample" ]; then
      case "$sample" in /*) ;; *) sample="$start_dir/$sample" ;; esac
      [ -f "$sample" ] || { echo "ts-try: the sample $sample is not a file" >&2; exit 2; }
    fi

    case "$(uname -s)" in
      Darwin)
        time_args=(-l)
        memory_name="peak memory footprint"
        ;;
      *)
        time_args=(-v)
        memory_name="max RSS"
        ;;
    esac

    # One build. The binary is the one cargo names on its `Executable` line: globbing target/ could find a stale one.
    build_log=$(mktemp)
    if ! cargo test -j2 --release -p typescript-analysis --test sweep --no-run >"$build_log" 2>&1; then
      tail -n 20 "$build_log" >&2
      rm -f "$build_log"
      echo "ts-try: the release build failed" >&2
      exit 1
    fi
    binary=$(sed -n 's/^ *Executable .*sweep\.rs (\(.*\))$/\1/p' "$build_log" | head -n 1)
    rm -f "$build_log"
    [ -n "$binary" ] || { echo "ts-try: cargo did not name the sweep binary (no 'Executable' line)" >&2; exit 1; }
    case "$binary" in /*) ;; *) binary="$PWD/$binary" ;; esac
    [ -x "$binary" ] || { echo "ts-try: $binary is not an executable" >&2; exit 1; }

    work=$(mktemp -d)
    failed=0
    trap '[ "$failed" = 1 ] || rm -rf "$work"' EXIT

    export TS_SWEEP_ROOT="$root" TS_SWEEP_FILE="$file"
    if [ -n "$sample" ]; then
      export TS_SWEEP_SAMPLE="$sample"
      what="the entries of $sample, whole"
    else
      export TS_SWEEP_DEPTH=1 TS_SWEEP_FILE_CODE=1
      what="every def and file, depth 1"
    fi

    # run <off|on> <round>: appends "<seconds> <megabytes>" to $work/<off|on>.txt
    run() {
      local layer=$1 round=$2 out="$work/$1-$2.out" stats="$work/$1-$2.time"
      local status=0
      if [ "$layer" = on ]; then export TS_SWEEP_PRECISE=1; else unset TS_SWEEP_PRECISE; fi
      /usr/bin/time "${time_args[@]}" -o "$stats" \
        "$binary" --ignored --exact --nocapture sweep_every_def_of_a_project >"$out" 2>&1 || status=$?
      if [ "$status" -ne 0 ]; then
        failed=1
        echo "ts-try: the sweep failed (layer $layer, round $round, exit $status); last lines of its output, which is kept in $out:" >&2
        tail -n 20 "$out" >&2
        exit 1
      fi
      awk -v darwin="$([ "$(uname -s)" = Darwin ] && echo 1 || echo 0)" '
        darwin == 1 && / real / { secs = $1 }
        darwin == 1 && /peak memory footprint/ { mb = $1 / 1048576 }
        darwin == 0 && /Elapsed \(wall clock\)/ {
          n = split($NF, part, ":"); secs = 0
          for (i = 1; i <= n; i++) secs = secs * 60 + part[i]
        }
        darwin == 0 && /Maximum resident set size/ { mb = $NF / 1024 }
        END { if (secs == "" || mb == "") exit 1; printf "%s %s\n", secs, mb }
      ' "$stats" >>"$work/$layer.txt" || {
        failed=1
        echo "ts-try: cannot read the time and memory from $stats:" >&2
        cat "$stats" >&2
        exit 1
      }
      rm -f "$out" "$stats"
    }

    echo "cost: $what; $rounds round(s), layer off then on, alternating (release build)"
    round=1
    while [ "$round" -le "$rounds" ]; do
      run off "$round"
      run on "$round"
      round=$((round + 1))
    done

    # median <column> <file>
    median() {
      awk -v col="$1" '{ v[NR] = $col } END {
        n = NR; for (i = 1; i <= n; i++) for (j = i + 1; j <= n; j++) if (v[j] + 0 < v[i] + 0) { t = v[i]; v[i] = v[j]; v[j] = t }
        print (n % 2) ? v[(n + 1) / 2] : (v[n / 2] + v[n / 2 + 1]) / 2
      }' "$2"
    }
    off_time=$(median 1 "$work/off.txt") off_mem=$(median 2 "$work/off.txt")
    on_time=$(median 1 "$work/on.txt") on_mem=$(median 2 "$work/on.txt")

    awk -v ot="$off_time" -v om="$off_mem" -v nt="$on_time" -v nm="$on_mem" -v mem="$memory_name" '
      BEGIN {
        tr = (ot > 0) ? nt / ot : 0; mr = (om > 0) ? nm / om : 0
        printf "\n%-6s %12s %20s\n", "layer", "wall (s)", mem " (MB)"
        printf "%-6s %12.2f %20.0f\n", "off", ot, om
        printf "%-6s %12.2f %20.0f\n", "on", nt, nm
        printf "%-6s %11.2fx %19.2fx\n", "on/off", tr, mr
        printf "(medians; memory is the %s)\n", mem
        if (tr > 2 || mr > 1.25) {
          which = (tr > 2 && mr > 1.25) ? "time and peak" : (tr > 2 ? "time" : "peak")
          printf "over the line to watch (2x time, +25%% peak): %s\n", which
        }
      }'
    ;;
  *) usage ;;
esac
