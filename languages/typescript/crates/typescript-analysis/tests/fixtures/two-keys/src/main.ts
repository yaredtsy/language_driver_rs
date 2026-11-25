// Type errors on purpose: neither `needs` nor `wants` fits `cb: () => void`. The checker reports them and the program still loads.
function run(cb: () => void) {
    cb();
    const bound = helper.bind(null);
}
function helper() {}
function needs(x: string) {}
function wants(x: number) {}
function main() {
    run(needs);
    run(wants);
}
