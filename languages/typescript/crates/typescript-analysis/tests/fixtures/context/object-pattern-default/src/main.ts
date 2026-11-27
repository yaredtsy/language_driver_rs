function save() {}
function fallback() {}
function open({ onDone = fallback }: { onDone?: () => void }) { onDone(); }
function main() {
    open({});
    open({ onDone: save });
}
