class Saver { save() { base(); } }
function base() {}
function mine() {}
function run(s: Saver) { s.save(); }
function main() {
    run({ save() { mine(); } });
}
