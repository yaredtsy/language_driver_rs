interface Opts { onDone: () => void }
function save() {}
function open(opts: Opts) { const o = opts; const { onDone } = o; onDone(); }
function main() {
    open({ onDone: save });
}
