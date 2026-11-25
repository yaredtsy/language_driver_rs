function run(cb: () => void = fallback) {
    cb();
}
function fallback() {}
function handler() {}
function main() {
    run();
    run(handler);
}
