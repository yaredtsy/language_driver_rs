// B6, B7: a named const or static is the enclosing def (joiner ::); const _ is transparent.
const _: () = {
    fn in_underscore() {}
    struct Hidden;
    impl Hidden { fn m() {} }
    impl Tr for Hidden { fn t() {} }
};
const C: u8 = { fn helper() {} 1 };
static X: u8 = { fn helper() {} 2 };
mod m {
    const _: () = { fn in_module() {} };
    const E: u8 = { fn in_module_const() {} 1 };
}
fn outer() {
    const _: () = { fn in_fn() {} };
    const D: u8 = { fn d_helper() {} 3 };
}
struct S;
impl S {
    const A: u8 = { fn in_assoc() {} 1 };
}
trait T {
    const B: u8 = { fn in_trait_const() {} 1 };
}
impl T for S {
    const B: u8 = { fn in_trait_impl_const() {} 2 };
}
const _: () = { const _: () = { fn twice() {} }; };
const _: () = { const INNER: u8 = { fn deep() {} 1 }; };
