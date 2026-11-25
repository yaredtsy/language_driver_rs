declare const flag: boolean;
function a() {}
function b() {}
function main() {
    const h = flag ? a : b;
    h();
}
