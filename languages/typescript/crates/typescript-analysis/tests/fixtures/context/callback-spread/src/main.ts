function run(a: () => void, b: () => void) {
    a();
    b();
}
function first() {}
function second() {}
function main() {
    const more: [() => void] = [second];
    run(first, ...more);
}
