interface Opts { onDone: () => void }
function save() {}
function fallback() {}
function change(o: Opts) { o.onDone = fallback; }
function look(o: Opts) { const { onDone } = o; if (onDone) {} }
function open(opts: Opts) { change(opts); const { onDone } = opts; onDone(); }
function looked(opts: Opts) { look(opts); const { onDone } = opts; onDone(); }
function main() {
    open({ onDone: save });
    looked({ onDone: save });
}
