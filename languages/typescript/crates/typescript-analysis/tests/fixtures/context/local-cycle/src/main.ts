function a() {}
function main() {
    let g: () => void = a;
    let h: () => void = g;
    g = h;
    h();
}
