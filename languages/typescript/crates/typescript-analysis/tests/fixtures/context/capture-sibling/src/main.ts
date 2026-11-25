declare function on(event: string, cb: () => void): void;
function log() {}
function setup(report: () => void) {
    function second() {
        report();
    }
    function first() {
        second();
    }
    on("click", first);
}
function main() {
    setup(log);
}
