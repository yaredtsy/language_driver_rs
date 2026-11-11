import fallback, { arrow, Counter, CsvWriter, generic, JsonWriter, overloaded, plain, reexported } from "./lib";
import type { Writer } from "./lib";
import * as lib from "./lib";
import { plain as renamed } from "./lib";
import { helper } from "pkg";
import { declared } from "./types";

declare const anyValue: any;
declare const key: string;

export function callers(counter: Counter, writer: Writer, either: JsonWriter | CsvWriter, cb: () => void) {
  plain();
  counter.bump();
  Counter.make();
  writer.write("x");
  either.write("y");
  overloaded("s");
  generic<number>(1);
  fallback();
  arrow();
  reexported();
  lib.plain();
  renamed();
  helper();
  declared();
  Math.max(1, 2);
  [1].map((n) => n);
  cb();
  anyValue.run();
  anyValue();
  (lib as any)[key]();
  counter["bump"]();
}
