function run(cb: () => void) {
    cb();
}
function handler() {}
function main() {
    const bound = handler.bind(null);
    run(bound);
}
