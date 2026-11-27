class Saver { save() { base(); } }
class Holder { inner: Saver = new Saver(); }
function base() {}
function mine() {}
function run(h: Holder) { h.inner.save(); }
function main() {
    run({ inner: { save() { mine(); } } });
}
