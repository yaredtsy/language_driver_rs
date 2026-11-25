function run(cb: () => void = fallback) {
    cb();
}
function fallback() {}
function handler() {}
function viaVoid(flag: boolean) {
    run(flag ? handler : void 0);
}
function viaUndefined(flag: boolean) {
    run(flag ? handler : undefined);
}
