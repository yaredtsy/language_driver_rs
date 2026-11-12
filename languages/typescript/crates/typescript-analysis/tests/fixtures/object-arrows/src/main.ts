import defaulted, { api, wrapped, documented, mutable, Holder, left, right } from "./apis";

declare const either: typeof left | typeof right;
declare function register(o: unknown): void;

export function run() {
  api.load();
  api["load"]();
  api.tasks.update("1");
  wrapped.a();
  documented.marked();
  mutable.a();
  defaulted.fromDefault();
  Holder.config.read();
  either.go();
  // a const object literal in a function body is a def of that function's locals
  const local = { l: () => other() };
  local.l();
  // a literal passed as an argument is not: its arrow's calls stay in `run`
  register({ onSuccess: () => other() });
}

function other() {}
