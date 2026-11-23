class Json { write() {} }
function take(x: unknown) {}
function takeN(n: number) {}
function takeFn(f: () => void) {}
function takeObj(o: { run: () => void }) {}
function takeW(w: Json) {}
function many(...fs: (() => void)[]) {}
function make(): () => void { return () => {}; }
function handler() {}
const config = { run: handler };
function Panel(props: { onClose: () => void; label: string }) { return null; }

export function callers(cb: () => void, props: { onDone: () => void }) {
  const local = handler;
  takeFn(handler);
  takeFn(() => {});
  takeObj({ run: handler });
  take({ n: 1 });
  takeFn(cb);
  takeFn(props.onDone);
  takeFn(local);
  take(config);
  takeW(new Json());
  const j = new Json();
  takeW(j);
  takeFn(make());
  take(Math.max(1, 2));
  takeN(5);
  take(`x`);
  <Panel onClose={handler} label="x" />;
  many(...[handler]);
  <Panel {...{ onClose: handler, label: "y" }} />;
}

export class Box {
  store = { run: handler };
  go() { take(this.store); }
}
