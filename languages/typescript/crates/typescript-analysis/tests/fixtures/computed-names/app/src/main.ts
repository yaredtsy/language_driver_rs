// A method whose name is computed, called: the engine's declaration is `[Symbol.iterator](): …` in a library, in the project's `.d.ts`, in a file outside the root
// and in the project's own interface and class. `tests/computed_names.rs` says what each gives.
import type { OutsideIterish } from "../../outside/iterish";
import type { ProjectDeclared } from "./shapes";

export function arrayIterator() {
  [1, 2][Symbol.iterator]();
}
export function asyncGenerator(g: AsyncGenerator<number>) {
  g[Symbol.asyncIterator]();
}
export function mapIterator(m: Map<string, number>) {
  m[Symbol.iterator]();
}
export function disposable(d: Disposable) {
  d[Symbol.dispose]();
}
export function inProjectDts(i: ProjectDeclared) {
  i[Symbol.iterator]();
}
export function outsideRoot(i: OutsideIterish) {
  i[Symbol.iterator]();
}
interface Local {
  [Symbol.iterator](): void;
}
export function inProjectInterface(i: Local) {
  i[Symbol.iterator]();
}
class K {
  [Symbol.iterator]() {
    return 1 as never;
  }
}
export function viaClass(k: K) {
  k[Symbol.iterator]();
}
