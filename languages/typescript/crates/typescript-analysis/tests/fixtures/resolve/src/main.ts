import fallback, {
  api,
  arrow,
  ArrowA,
  ArrowB,
  Child,
  Counter,
  CsvWriter,
  generic,
  JsonWriter,
  makeHandler,
  overloaded,
  plain,
  reexported,
  second,
} from "./lib";
import type { Reader, Writer } from "./lib";
import * as lib from "./lib";
import { plain as renamed } from "./lib";
import { helper, toolbox, useThing } from "pkg";
import { declared } from "./types";

declare const anyValue: any;
declare const key: string;

export function callers(
  counter: Counter,
  writer: Writer,
  either: JsonWriter | CsvWriter,
  cb: () => void,
  arrows: ArrowA | ArrowB,
  maybe: JsonWriter | CsvWriter | undefined,
  child: Child,
  reader: Reader,
) {
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
  either["write"]("z");
  arrows.write("a");
  maybe?.write("m");
  child.bump();
  api.load();
  reader.read(1);
}

type Result = { kind: "idle"; mutate: () => void } | { kind: "busy"; mutate: () => void };

export function optional_callbacks(cb: (() => void) | undefined, result: Result) {
  cb?.();
  result.mutate();
}

export function value_holders({ onSave }: { onSave: () => void }) {
  let later = () => {};
  later();
  onSave();
}

export function no_symbol(flag: boolean) {
  makeHandler()();
  (flag ? plain : second)();
}

export function external_value_holders() {
  useThing();
  toolbox.run();
}

export function twice() {
  plain();
  plain();
}
