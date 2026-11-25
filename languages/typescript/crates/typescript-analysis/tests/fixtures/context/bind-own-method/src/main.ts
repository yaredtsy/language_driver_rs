function other() {}
function handler() {}
namespace handler {
    export function bind(_: unknown): () => void {
        return other;
    }
}
function run(cb: () => void) {
    cb();
}
function main() {
    run(handler.bind(null));
}
