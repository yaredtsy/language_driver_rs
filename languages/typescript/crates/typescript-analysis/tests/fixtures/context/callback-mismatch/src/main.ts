// A type error on purpose: `needs` takes two parameters and `run` passes none. The checker reports it and the program still loads.
function run(cb: () => void) {
    cb();
}
function needs(a: string, b: number) {}
function main() {
    run(needs);
}
