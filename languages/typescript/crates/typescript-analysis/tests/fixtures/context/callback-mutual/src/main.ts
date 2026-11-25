function ping(n: number, cb: () => void) {
    cb();
    if (n > 0) {
        pong(n - 1, cb);
    }
}
function pong(n: number, cb: () => void) {
    cb();
    if (n > 0) {
        ping(n - 1, cb);
    }
}
function handler() {}
function main() {
    ping(2, handler);
}
