import { fa } from "./a";
import { fb } from "./b";
function leaf() {}
function run(cb: (g: () => void) => void) {
    cb(leaf);
}
export function pick(flag: boolean) {
    run(flag ? fb : fa);
}
export function touchA() {
    fa(leaf);
}
