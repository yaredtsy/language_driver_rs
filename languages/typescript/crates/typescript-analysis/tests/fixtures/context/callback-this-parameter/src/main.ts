function run(this: void, cb: () => void) {
    cb();
}
function handler() {}
function main() {
    run(handler);
}
