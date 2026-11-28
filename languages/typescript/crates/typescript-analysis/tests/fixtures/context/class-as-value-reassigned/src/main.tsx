function encode() {}
function other() {}
class TextWriter { constructor() { other(); } }
function main(flag: boolean) { let C = class X { constructor() { encode(); } }; if (flag) C = TextWriter; return new C(); }
