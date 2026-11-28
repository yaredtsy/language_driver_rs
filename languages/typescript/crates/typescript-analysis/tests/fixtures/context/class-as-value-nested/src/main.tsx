function encode() {}
function register(C: new () => object) { return new C(); }
function nested(cb: () => void) { class W { constructor() { cb(); } } register(W); }
function main() { nested(encode); }
