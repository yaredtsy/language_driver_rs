function a() {}
function b() {}
function main() {
    let h = a;
    h();
    h = b;
    h();
}
