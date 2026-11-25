class Runner {
    run(cb: () => void) {
        cb();
    }
}
function handler() {}
function main() {
    new Runner().run(handler);
}
