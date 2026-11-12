function a() {}
function b() {}
function c() {}
// an instance initializer, a static initializer and a static block are the class's own body
export class Body {
  x = a();
  static s = b();
  static {
    c();
  }
  method() {}
}
// a construction draws the constructor, not the body (counted: part A)
export function makes() {
  new Body();
}
