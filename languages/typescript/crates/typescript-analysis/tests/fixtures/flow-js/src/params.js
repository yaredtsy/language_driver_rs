// The parameter table of untyped JavaScript (TC2 step 2): an untyped parameter is `any`, so capable.
export function each(f) {
  f();
}
export function withDefault(f = () => {}) {
  f();
}
