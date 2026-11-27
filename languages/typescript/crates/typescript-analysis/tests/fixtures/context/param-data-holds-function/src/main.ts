function handler() {}
function noop() {}
function lied(cb: string, other: () => void) {
    ((cb as any) || other)();
}
function liar() {
    lied(handler as any, noop);
}
function honest() {
    lied("x", noop);
}
