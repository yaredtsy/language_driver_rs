import { thing } from "./thing";
// the `.d.ts` beside the `.js` wins in the checker
export function viaShadow() {
  thing();
}
