// S-BD4, the Ref oracle (docs/08-bend/02-phase-1-parsing.md §1), deferred here from BD0: for the book of each entry file, every `Ref` that Bend's own
// loader leaves in the body of a def, with the span it was written at. It is BD4's oracle for call sites, and BD3's for names: every callee below must
// be found by our `Book::resolve`.
//
//   bun scripts/bend-refs.ts <path to the bend clone> <out dir> [entry.bend ...]
//
// With no entry, every `.bend` directly under `<clone>/demos/<demo>/`. Run it from anywhere but inside a crate (`languages/bend/crates/*`). It writes
// `<out dir>/refs/<entry path relative to the clone, `/` as `__`>.tsv`, one line per Ref, and nothing else outside <out dir>:
//
//   <file> TAB <def ns:name> TAB <callee ns:name> TAB <beg>-<end>
//
// <file>      where the Ref was written, as a path from the entry file's directory (`PROOF.bend`, `../lib.bend`)
// <def>       the def whose body holds it, named as Bend names it: a def that fills a law of another file is `<law's namespace>:<name>`, and its
//             <file> is the one the proof is written in
// <callee>    what `parse_reso` resolved the name to: `ns:name`, or the bare name for Base's and for the entry file's own (its namespace is "")
// <beg>-<end> a UTF-16 code unit range into the text of <file> as it is on disk (JavaScript string indices, not bytes); for a Ref that Bend makes up
//             (`(a + b : T)` is `T.add`, `a[i]` is `Array.get`, `3n+p` is `Nat.add`) it covers the whole sugar, not a name. Bend itself parses the text
//             with its `import` lines emptied (`book_load`: `body[i] = ""`), so its own offsets sit before the file's by the length of every import line
//             above; the script puts that length back
//
// A body is walked after `term_lower` (which opens every binder), once per distinct (span, callee); the lines of a def are sorted by span. A Ref in a
// type position is in the list too (the body's annotations, `x : List<T>`): the policy that tells types from code is BD4's, not Bend's.
// Defs without code (a type, a law with no proof, a foreign def's leaf) have no lines.
//
// Like `bend-corpus.sh`: telemetry off, the hub a closed port, the package folder under <out dir>; nothing is fetched, nothing under ~/.bend is read.
import * as fs from "node:fs";
import * as path from "node:path";

const [clone, out, ...given] = process.argv.slice(2);
if (clone === undefined || out === undefined) {
  console.error("usage: bun scripts/bend-refs.ts <path to the bend clone> <out dir> [entry.bend ...]");
  process.exit(2);
}
if (!fs.existsSync(path.join(clone, "bend2/bend.ts"))) {
  console.error(clone + " is not a bend clone (no bend2/bend.ts)");
  process.exit(2);
}
fs.mkdirSync(path.join(out, "lib"), { recursive: true });
fs.mkdirSync(path.join(out, "refs"), { recursive: true });
// bend.ts reads these at load time, so they are set before the import
process.env.BEND_NO_TELEMETRY = "1";
process.env.BEND_LIB = fs.realpathSync(path.join(out, "lib"));
process.env.BEND_HUB = "http://127.0.0.1:9";
const Bend: any = await import(path.resolve(clone) + "/bend2/bend.ts");

const entries = given.length > 0 ? given : fs.readdirSync(path.join(clone, "demos")).sort().flatMap((demo) =>
  fs.readdirSync(path.join(clone, "demos", demo)).filter((f) => f.endsWith(".bend")).sort().map((f) => path.join(clone, "demos", demo, f)));

// the Refs under a lowered term, once each. A node can be reachable twice (Bend shares subterms), and a span is the identity of a use.
type Found = { callee: string; beg: number; end: number; ns: string; str: string };
const refs = (term: any): Found[] => {
  const found = new Map<string, Found>();
  const seen = new Set<object>();
  const walk = (x: any): void => {
    if (typeof x !== "object" || x === null || seen.has(x)) {
      return;
    }
    seen.add(x);
    if (Array.isArray(x)) {
      x.forEach(walk);
      return;
    }
    if (x.$ === "Ref" && x.s !== undefined) {
      found.set(x.s.file.ns + "\0" + x.s.beg + "\0" + x.k, { callee: x.k, beg: x.s.beg, end: x.s.end, ns: x.s.file.ns, str: x.s.file.str });
    }
    for (const [key, value] of Object.entries(x)) {
      if (key !== "s" && key !== "c") {
        walk(value);
      }
    }
  };
  walk(term);
  return [...found.values()].sort((a, b) => a.beg - b.beg || (a.callee < b.callee ? -1 : 1));
};

// the offset in the text Bend parsed -> the offset in the file: add back the import lines (emptied by Bend) that lie before it
const unblank = (real: string, blank: string): ((at: number) => number) => {
  const was = real.split("\n");
  const now = blank.split("\n");
  const starts: number[] = [];
  const lost: number[] = [];
  for (let i = 0, at = 0, gone = 0; i < now.length; at += now[i].length + 1, gone += was[i].length - now[i].length, i++) {
    starts.push(at);
    lost.push(gone);
  }
  return (at) => {
    let i = starts.length - 1;
    while (i > 0 && starts[i] > at) {
      i--;
    }
    return at + lost[i];
  };
};

let total = 0;
for (const entry of entries) {
  const file = fs.realpathSync(entry);
  const name = path.relative(fs.realpathSync(clone), file).split(path.sep).join("__");
  const book = Bend.book_nil();
  try {
    await Bend.book_load(book, file, "", new Map());
  } catch (e: any) {
    console.log(name + ": the loader refuses (" + String(e?.exp ?? e).split("\n")[0].slice(0, 100) + ")");
    continue;
  }
  const lines: string[] = [];
  const done = new Set<string>();
  const maps = new Map<string, (at: number) => number>();
  for (const k of book.order) {
    const tld = book.tlds[k];
    // a law and its proof are one tld listed twice, and a def of Base is not ours
    if (done.has(k) || tld.$ !== "Def" || tld.b === true || tld.v === null) {
      continue;
    }
    done.add(k);
    for (const r of refs(Bend.term_lower(tld.v))) {
      // namespace "" is the entry file's: Base's own defs are skipped above
      const where = r.ns === "" ? path.basename(file) : r.ns + ".bend";
      if (!maps.has(r.ns)) {
        maps.set(r.ns, unblank(fs.readFileSync(path.join(path.dirname(file), where), "utf8"), r.str));
      }
      const at = maps.get(r.ns)!;
      lines.push([where, k, r.callee, at(r.beg) + "-" + at(r.end)].join("\t"));
    }
  }
  fs.writeFileSync(path.join(out, "refs", name + ".tsv"), lines.map((l) => l + "\n").join(""));
  console.log(name + ": " + lines.length + " refs, " + done.size + " defs with code");
  total += lines.length;
}
console.log("total: " + total + " refs");
