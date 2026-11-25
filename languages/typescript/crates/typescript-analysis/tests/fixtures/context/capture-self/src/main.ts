declare function later(cb: () => void): void;
function main() {
    let next: () => void = () => {};
    function tick() {
        later(next);
    }
    next = tick;
    later(tick);
}
