export class JsonWriter {
  write(s: string) {}
}
export class CsvWriter {
  write(s: string) {}
}
export function viaUnion(w: JsonWriter | CsvWriter) {
  w.write("x");
}
// a member that is a function type has nothing to enter: the answer says it is not complete
export function viaUnionWithFunction(w: JsonWriter | { write: (s: string) => void }) {
  w.write("x");
}
