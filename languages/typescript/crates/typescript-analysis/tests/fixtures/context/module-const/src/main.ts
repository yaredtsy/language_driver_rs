declare const pick: boolean;
function a() {}
function b() {}
const onSave = pick ? a : b;
function main() {
    onSave();
}
