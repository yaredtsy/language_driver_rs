function run(...cbs: Array<() => void>) {
    const [first] = cbs;
    first();
}
function handler() {}
function main() {
    run(handler);
}
