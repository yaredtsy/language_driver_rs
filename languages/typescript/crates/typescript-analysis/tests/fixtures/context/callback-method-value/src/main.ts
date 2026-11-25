class Api {
    load() {
        fetchAll();
    }
}
function fetchAll() {}
function run(cb: () => void) {
    cb();
}
function main() {
    const api = new Api();
    run(api.load);
}
