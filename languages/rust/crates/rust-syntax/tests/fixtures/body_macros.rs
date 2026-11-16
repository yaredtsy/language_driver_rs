// B3: a macro_rules! in a function body is outer::{fn}::m!, like any other item of the body.
fn outer() {
    macro_rules! local { () => {}; }
    fn inner() { macro_rules! deeper { () => {}; } }
}
impl S {
    fn method(&self) { if true { macro_rules! in_block { () => {}; } } }
}
mod m {
    fn f() { macro_rules! in_module_fn { () => {}; } }
}
