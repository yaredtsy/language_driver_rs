function run(cb: () => void) {
    cb();
}
function handler() {}
function main() {
    const h = handler;
    run(h);
}
