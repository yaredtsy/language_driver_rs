function encode() {}
function other() {}
class JsonWriter { constructor() { encode(); } }
class TextWriter { constructor() { other(); } }
function main(flag: boolean) { const C = flag ? JsonWriter : TextWriter; return new C(); }
