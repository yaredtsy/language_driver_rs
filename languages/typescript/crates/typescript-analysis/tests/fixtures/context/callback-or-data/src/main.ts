function run(cb: (() => void) | false) {
    if (cb) {
        cb();
    }
}
function handler() {}
function main(ok: boolean) {
    run(ok && handler);
}
