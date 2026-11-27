function save() {}
function open(opts: { label: string; onDone: () => void }) { const { label, ...rest } = opts; const { onDone } = rest; onDone(); }
function main() {
    open({ label: "x", onDone: save });
}
