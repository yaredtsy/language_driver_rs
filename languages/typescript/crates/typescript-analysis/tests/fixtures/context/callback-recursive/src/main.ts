function walk(n: number, visit: () => void) {
    visit();
    if (n > 0) {
        walk(n - 1, visit);
    }
}
function handler() {}
function main() {
    walk(3, handler);
}
