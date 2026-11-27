class Saver { save() { base(); } }
function base() {}
function a() {}
function b() {}
function main(flag: boolean) {
    let s: Saver = { save() { a(); } };
    if (flag) {
        s = { save() { b(); } };
    }
    s.save();
}
