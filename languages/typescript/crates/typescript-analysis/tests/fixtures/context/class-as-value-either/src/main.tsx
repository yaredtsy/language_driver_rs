function encode() {}
function other() {}
class JsonWriter { constructor() { encode(); } }
class TextWriter { constructor() { other(); } }
function register(C: new () => object) { return new C(); }
function main(flag: boolean) { register(flag ? JsonWriter : TextWriter); }
