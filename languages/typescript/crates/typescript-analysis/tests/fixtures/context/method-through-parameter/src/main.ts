function save() {}
function other() {}
function poke(t: { reset(): void }) {
    t.reset();
}
function main() {
    const o = { f: save, reset() { this.f = other; } };
    poke(o);
    o.f();
}
