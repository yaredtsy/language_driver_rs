function a() {}
function b() {}
let handler: () => void = a;
export function setHandler() {
    handler = b;
}
function main() {
    handler();
}
