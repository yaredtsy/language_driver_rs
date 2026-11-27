interface Opts { onDone: () => void }
function save() {}
function open(opts: Opts) { const box = { inner: opts }; const { onDone } = opts; onDone(); }
function main() {
    open({ onDone: save });
}
