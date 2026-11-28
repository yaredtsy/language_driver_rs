function encode() {}
function other() {}
class JsonWriter { constructor() { encode(); } }
class Sub extends JsonWriter { constructor() { super(); other(); } }
function build(C: typeof JsonWriter) { return new C(); }
function main() { build(JsonWriter); build(Sub); }
