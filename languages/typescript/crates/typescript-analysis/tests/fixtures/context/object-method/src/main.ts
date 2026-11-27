function save() {}
function main() {
    const o = { run() { save(); } };
    o.run();
}
