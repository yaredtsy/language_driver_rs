import { helper } from "pkg";
import { shimmed } from "./shim";
// a package, the repo's own `.d.ts`, a bundled library: each an `External` edge, never entered
export function external() {
  helper();
  shimmed();
  Math.max(1, 2);
}
