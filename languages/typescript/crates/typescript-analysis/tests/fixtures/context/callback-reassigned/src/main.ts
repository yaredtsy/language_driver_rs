function run(cb: () => void) {
    cb = cb || fallback;
    cb();
}
function handler() {}
function fallback() {}
function main() {
    run(handler);
}
