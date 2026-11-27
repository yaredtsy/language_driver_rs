interface Opts { onDone: () => void }
function save() {}
function open(opts: Opts) { const all = [opts]; const { onDone } = opts; onDone(); }
function main() {
    open({ onDone: save });
}
