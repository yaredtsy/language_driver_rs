interface Props {
  onSave: () => void;
}
class Store {
  constructor(public handler: () => void) {}
  run() {
    this.handler();
  }
}
// a prop, a field of function type, a variable that holds an arrow (`let`)
export function holders(p: Props, s: Store) {
  p.onSave();
  s.run();
  let later = () => {};
  later();
}
