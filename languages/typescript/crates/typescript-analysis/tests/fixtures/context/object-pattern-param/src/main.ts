interface Opts { onDone: () => void }
function save() {}
function open({ onDone }: Opts) { onDone(); }
function main() {
    open({ onDone: save });
}
