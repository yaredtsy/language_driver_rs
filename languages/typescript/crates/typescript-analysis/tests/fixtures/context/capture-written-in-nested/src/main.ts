declare function later(cb: () => void): void;
function a() {}
function b() {}
function main() {
    let h: () => void = a;
    const swap = () => {
        h = b;
    };
    const fire = () => {
        h();
    };
    later(swap);
    later(fire);
}
