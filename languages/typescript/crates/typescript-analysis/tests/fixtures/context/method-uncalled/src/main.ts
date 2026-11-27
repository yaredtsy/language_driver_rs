function save() {}
function other() {}
function run(o: { f: () => void; reset(): void }) {
    o.f();
}
function main() {
    run({ f: save, reset() { this.f = other; } });
}
