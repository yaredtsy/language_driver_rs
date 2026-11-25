// Type errors on purpose: neither `needs` nor `wants` fits `() => void`. The checker reports them and the program still loads.
function run(cb: () => void) {
    cb();
}
function needs(a: string, b: number) {}
function wants(a: string, b: string) {}
function main(flag: boolean) {
    run(flag ? needs : wants);
}
