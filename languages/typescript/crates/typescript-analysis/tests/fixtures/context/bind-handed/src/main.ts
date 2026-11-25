function run(cb: () => void) {
    cb();
}
function handler() {}
function main() {
    run(handler.bind(null));
}
