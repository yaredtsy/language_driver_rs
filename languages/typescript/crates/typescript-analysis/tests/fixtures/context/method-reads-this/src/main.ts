function save() {}
function other() {}
function run(o: { f: () => void; reset(): void }) {
    o.reset();
    o.f();
}
function main() {
    run({ f: save, reset() { if (this.f) {} } });
}
