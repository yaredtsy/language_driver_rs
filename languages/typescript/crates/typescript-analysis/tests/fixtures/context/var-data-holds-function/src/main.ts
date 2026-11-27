function handler() {}
function main() {
    const s: string = handler as any;
    (s as any)();
}
