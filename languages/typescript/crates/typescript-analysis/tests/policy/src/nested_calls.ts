export function leaf() {}
// a named nested function is its own def, drawn where it is called
export function nested() {
  function inner() {
    leaf();
  }
  inner();
  inner();
}
// and not where it is declared
export function declaredNotCalled() {
  function unused() {
    leaf();
  }
}
