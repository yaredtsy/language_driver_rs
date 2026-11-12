export interface Writer {
  write(s: string): void;
}
export class JsonWriter implements Writer {
  write(s: string) {}
}
// the interface's method signature, not the implementations
export function viaInterface(w: Writer) {
  w.write("x");
}
