function encode() {}
class JsonWriter { constructor() { encode(); } }
function make(o: { W: new () => object }) { return new o.W(); }
function main() { make({ W: JsonWriter }); }
