function save() {}
function run(o: { f: () => void; reset(): void }) {
    o.reset();
    o.f();
}
function main() {
    const reset = () => {};
    run({ f: save, reset });
}
