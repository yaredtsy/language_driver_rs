export class Counter {
  bump() {}
}
// a computed name is unknown; a name the checker narrows to a literal is resolved
export function byName(c: Counter, key: string) {
  // @ts-ignore
  c[key]();
  c["bump"]();
}
