export class JsonWriter {
  write(s: string) {}
}
export class Base {
  run() {}
}
export class Derived extends Base {}
// the class's method, or the base class's where it is declared
export function onClass(w: JsonWriter, d: Derived) {
  w.write("x");
  d.run();
}
