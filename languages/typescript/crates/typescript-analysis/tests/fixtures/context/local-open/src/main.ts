function a() {}
function main(items: (() => void)[]) {
    let h: () => void = a;
    for (h of items) {}
    h();
}
