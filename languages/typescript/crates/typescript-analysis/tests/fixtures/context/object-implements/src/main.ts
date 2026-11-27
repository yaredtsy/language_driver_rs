interface Writer { write(): void }
function encode() {}
function emit(w: Writer) { w.write(); }
function main() {
    emit({ write() { encode(); } });
}
