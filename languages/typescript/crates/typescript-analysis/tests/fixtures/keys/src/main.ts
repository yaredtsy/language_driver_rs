function run(cb: () => void) {
    cb();
}
function a() {}
function b() {}
function opt(cb?: () => void) {
    cb?.();
}
function withDefault(cb: () => void = a) {
    cb();
}
function many(...cbs: (() => void)[]) {
    for (const cb of cbs) {
        cb();
    }
}
function second(label: string, cb: () => void) {
    cb();
}
function tagged(parts: TemplateStringsArray, cb: () => void) {
    cb();
}
function ignore(cb: () => void) {
    console.log("x");
}
class Box {
    constructor(cb: () => void) {
        cb();
    }
}
class Sub extends Box {}
class Pair extends Box {
    constructor(n: number, cb: () => void) {
        super(cb);
    }
}
const list: [() => void] = [a];
function notPassed() {
    opt();
}
function defaulted() {
    withDefault();
}
function either(flag: boolean) {
    run(flag ? a : b);
}
function spread() {
    run(...list);
}
function rest() {
    many(a);
}
function leading() {
    second("x", a);
}
function template() {
    tagged`${a}`;
}
function arrowOne() {
    run(() => {});
}
function arrowTwo() {
    run(() => a());
}
function constructs() {
    new Box(a);
    new Sub(b);
    new Pair(1, a);
}
function ignored() {
    ignore(a);
    ignore(b);
}
function main() {
    notPassed();
    defaulted();
    either(true);
    spread();
    rest();
    leading();
    template();
    arrowOne();
    arrowTwo();
    constructs();
    ignored();
}
