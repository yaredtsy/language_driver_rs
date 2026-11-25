import { onSave } from "./handlers";
function run(cb: () => void) {
    cb();
}
function main() {
    run(onSave);
}
