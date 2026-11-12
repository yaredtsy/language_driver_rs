import dflt from "./lib";
import * as ns from "./lib";
import { renamed } from "./reexport";
import { real as alias } from "./lib";
// followed to the real declaration: the default export, a namespace import, a re-export under another name, an alias
export function imports() {
  dflt();
  ns.real();
  renamed();
  alias();
}
