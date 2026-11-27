function save() {}
function other() {}
function run(o: { f: () => void; reset(): void; clear(): void }) {
    o.reset();
    o.f();
}
function main() {
    run({ f: save, reset() { this.clear(); }, clear() { this.f = other; } });
}
