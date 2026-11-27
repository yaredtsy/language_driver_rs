interface Opts { onDone: () => void }
function save() {}
function open(opts: Opts) {
    const done = opts.onDone;
    done();
    opts.onDone();
}
function main() {
    open({ onDone: save });
}
