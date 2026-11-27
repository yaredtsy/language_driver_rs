function save() {}
declare function makeReset(): () => void;
function run(o: { f: () => void; reset(): void }) {
    o.reset();
    o.f();
}
function main() {
    run({ f: save, reset: makeReset() });
}
