declare function later(cb: () => void): void;
function done() {}
function outer(cb: () => void) {
    function middle() {
        function inner() {
            cb();
        }
        later(inner);
    }
    later(middle);
}
function main() {
    outer(done);
}
