interface Opts { onDone: () => void }
function save() {}
function open(opts: Opts) { console.log(opts); const { onDone } = opts; onDone(); }
function main() {
    open({ onDone: save });
}
