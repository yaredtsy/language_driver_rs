// B8: the same name in two blocks, or in a block and a closure, of one function has one qualname; another function's is its own.
fn outer() {
    { fn helper() {} }
    { fn helper() {} }
    let c = || { fn helper() {} };
}
fn other() { fn helper() {} }
