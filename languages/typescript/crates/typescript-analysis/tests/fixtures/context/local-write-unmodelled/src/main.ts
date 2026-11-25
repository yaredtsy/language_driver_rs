function a() {}
function b() {}
function main() {
    let h: () => void = a;
    (h) = b;
    h();
}
