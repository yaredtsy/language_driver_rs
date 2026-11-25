function run(cb: () => void): void;
function run(cb: () => void, times: number): void;
function run(cb: () => void, times?: number) {
    cb();
}
function handler() {}
function main() {
    run(handler);
    run(handler, 2);
}
