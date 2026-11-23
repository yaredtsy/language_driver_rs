declare namespace JSX { interface Element {} interface IntrinsicElements { button: { onClick?: () => void } } }

interface Writer { write(): void }
class Json implements Writer { write() { encode(); } }
class Csv  implements Writer { write() { join(); } }
function encode() {}
function join() {}

function each(f: () => void) { f(); }
function relay(f: () => void) { each(f); }
function open(opts: { onDone: () => void }) { opts.onDone(); }
function Panel({ onClose }: { onClose: () => void }) {
  const back = () => { onClose(); };
  return <button onClick={back} />;
}
function send(w: Writer) { w.write(); }
function pick(): Writer { return new Csv(); }
function twice(f: () => void) { return () => { f(); f(); }; }

function main() {
  each(encode);
  relay(join);
  const h = encode; h();
  open({ onDone: encode });
  <Panel onClose={join} />;
  send(new Json());
  pick().write();
  twice(encode)();
  each(() => join());
}

export { Json, Csv, Writer, encode, join, each, send };
