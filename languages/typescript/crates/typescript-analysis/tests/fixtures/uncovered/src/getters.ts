// A call through a getter's result: `b.fn()` is one call edge, to the getter. The call of what it returns is the second call.
export class Holder {
  get fn(): () => void {
    return () => {};
  }
  run() {}
}
export class Components {
  get Panel(): new () => object {
    return class {};
  }
}

// 1
export function callsResult(b: Holder) {
  b.fn();
}
// 2
export function callsResultTwice(b: Holder) {
  b.fn();
  b.fn();
}
// a method: 0
export function callsMethod(b: Holder) {
  b.run();
}
// a read of the getter is an accessor site, not a call of its result: 0
export function readsGetter(b: Holder) {
  const f = b.fn;
}
// the result is constructed, not called: 1
export function constructsResult(c: Components) {
  new c.Panel();
}
// a getter of a class the project does not hold (a repo .d.ts): an `External` edge, and the result is called: 1
export function callsExternalResult(e: Foreign) {
  e.callback();
}
// a getter of an anonymous type declared in a repo `.d.ts`: 1
export function callsAnonymousExternalResult() {
  holder.callback();
}
