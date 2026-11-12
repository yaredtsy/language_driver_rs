export function leaf() {}
export class Widget {
  m() {}
  static s() {}
}
// a call, an optional call, a static method, `new`, a tagged template: sites. A type position (`typeof leaf`) is not
export function sites(k: Widget) {
  leaf();
  k.m();
  k?.m();
  Widget.s();
  new Widget();
  leaf`x`;
  let later: typeof leaf;
}
