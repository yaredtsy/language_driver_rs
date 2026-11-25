function a() {}
function make(): () => void {
    return a;
}
const handler = make();
function main() {
    handler();
}
export {};
