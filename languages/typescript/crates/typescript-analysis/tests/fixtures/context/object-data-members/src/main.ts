function save() {}
function report(meta: Record<string, unknown>) {
    console.log(meta);
}
function main() {
    report({ a: 1 });
    report({ b: 2 });
    report({ c: 3, f: save });
}
