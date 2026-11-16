// B1, B2, B4: an item in a function body is named after the function, then {fn}. A local impl or trait keeps its own naming; a method is a function too.
fn outer() {
    fn inner() {}
    struct S;
    enum E { A }
    union U { a: u8 }
    type Alias = u8;
    const C: u8 = 1;
    static X: u8 = 2;
    /// A documented local item: its range starts at the doc line.
    #[inline]
    fn documented() {}
    trait T { fn m(&self); fn d(&self) {} }
    impl S { fn m(&self) {} }
    impl T for S { fn m(&self) {} }
    mod m { fn f() {} }
    extern "C" { fn ext(); }
    fn level_one() { fn level_two() { struct Deep; } }
}
struct Q;
impl Q {
    fn method(&self) { fn in_method() {} }
}
trait Tr {
    fn default_method(&self) { fn in_default() {} }
}
impl Tr for Q {
    fn default_method(&self) {
        fn in_trait_impl() {}
        struct Local;
        impl Local { fn m() {} }
    }
}
mod outer_mod {
    fn f() { fn g() {} }
}
