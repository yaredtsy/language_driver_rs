declare function later(cb: () => void): void;
declare function each(xs: number[], cb: (x: number) => void): void;
function done() {}
function outer(cb: () => void) {
    const inner = () => {
        each([1], () => cb());
    };
    later(inner);
}
function main() {
    outer(done);
}
