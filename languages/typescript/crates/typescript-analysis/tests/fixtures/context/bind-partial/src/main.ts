function run(cb: () => void) {
    cb();
}
function handler(x: number) {}
function main() {
    run(handler.bind(null, 1));
}
