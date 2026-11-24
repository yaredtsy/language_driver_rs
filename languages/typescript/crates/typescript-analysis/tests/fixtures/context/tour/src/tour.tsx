// The tour of the context layer (docs/03-typescript/12-context-layer/03-design.md §2): one program, through every phase.
// `main` is the program; each `lineX` holds one of its lines alone, so each line has a tree of its own.

interface Writer { write(): void }
class Json implements Writer { write() { encode(); } }
class Csv  implements Writer { write() { join(); } }
function encode() {}
function join() {}

function each(f: () => void) { f(); }                             // a callback parameter
function relay(f: () => void) { each(f); }                        // passed through
function open(opts: { onDone: () => void }) { opts.onDone(); }    // an options bag
function Panel({ onClose }: { onClose: () => void }) {            // props, and a nested handler that captures one
  const back = () => { onClose(); };
  return <button onClick={back} />;
}
function send(w: Writer) { w.write(); }                           // an interface parameter
function pick(): Writer { return new Csv(); }                     // a factory
function twice(f: () => void) { return () => { f(); f(); }; }     // returns a function

function main() {
  each(encode);                 // a
  relay(join);                  // b
  const h: () => void = encode; h();   // c
  open({ onDone: encode });     // d
  <Panel onClose={join} />;     // e
  send(new Json());             // f
  pick().write();               // g
  twice(encode)();              // h
  each(() => join());           // i
}

function lineA() { each(encode); }
function lineB() { relay(join); }
function lineC() { const h: () => void = encode; h(); }
function lineD() { open({ onDone: encode }); }
function lineE() { <Panel onClose={join} />; }
function lineF() { send(new Json()); }
function lineG() { pick().write(); }
function lineH() { twice(encode)(); }
function lineI() { each(() => join()); }
