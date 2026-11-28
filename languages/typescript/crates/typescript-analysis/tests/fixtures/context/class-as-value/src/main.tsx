function encode() {}
class JsonWriter { constructor() { encode(); } }
function register(C: new () => object) { return new C(); }
function main() { register(JsonWriter); }
