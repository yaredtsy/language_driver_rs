interface Opts { onDone: () => void }
function save() {}
function open(opts: Opts) { const { onDone } = opts; onDone(); }
function main() {
    open({ onDone: save });
}
