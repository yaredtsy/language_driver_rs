function save() {}
function runTwo(o: { a: { f: () => void } }) { const { a: { f } } = o; f(); }
function runThree(o: { a: { b: { c: () => void } } }) { const { a: { b: { c } } } = o; c(); }
function main() {
    runTwo({ a: { f: save } });
    runThree({ a: { b: { c: save } } });
}
