import { extLib } from "fakelib";

function use(o: { run: () => void }) {}
function handler() {}
function Panel(props: { item: object }) { return null; }

export const shared = { run: handler };
const hidden = { run: handler };
hidden.run = handler;

export function objects(p: { run: () => void }, q: { run: () => void }, cb: () => void) {
  const a = { run: handler };
  a.run();
  const b = { run: handler };
  b.run = handler;
  const c = { run: handler };
  use(c);
  const d = { run: handler };
  extLib(d);
  const e = { run: handler };
  const deps = [e];
  const f = { run: handler };
  const g = f;
  const h = { run: handler };
  const i = { run: handler };
  <Panel item={i} />;
  Object.assign(p, { run: handler });
  use(q);
  cb();
  void deps;
  void g;
  return h;
}
