interface Opts { onDone: () => void }
function save() {}
function fallback() {}
function open(opts: Opts) { const o = opts; o.onDone = fallback; const { onDone } = opts; onDone(); }
function main() {
    open({ onDone: save });
}
