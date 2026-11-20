use crate::shapes::{E, Pair, Rec, Unit};

/// A project macro defined in another module: its body's call is a `MacroBody` site at the macro call, and the argument is a call at its place.
pub fn macro_from_another_module() {
    from_elsewhere!(helper());
}

pub(crate) fn helper() -> u8 {
    1
}

fn twice(x: u8) -> u8 {
    x
}

const fn helper_const() -> u8 {
    1
}

pub struct Holder {
    pub cb: fn(u8) -> u8,
}

pub struct Wrapper(u8);

pub fn plain_calls() -> u8 {
    twice(helper())
}

pub fn path_calls() {
    crate::shapes::Rec::mk();
    self::helper();
}

pub fn method_chain(r: &Rec) -> u8 {
    r.value().checked_add(1).unwrap_or(0)
}

pub fn constructions() {
    Pair(1, 2);
    E::V(3);
    Rec { a: 4 };
    E::R { a: 5 };
    // a unit struct and a unit variant named as values: no call in the text or in the compiler
    let _unit = Unit;
    let _unit_variant = E::U;
}

impl Wrapper {
    pub fn make() -> Self {
        Self(1)
    }

    pub fn from_record() -> Rec {
        Rec { a: helper() }
    }
}

pub fn callee_expressions(h: &Holder, f: &dyn Fn(u8) -> u8) {
    (h.cb)(1);
    f(2);
    (helper)();
}

pub fn closures() {
    let a = || helper();
    let b = |x: u8| twice(x) + (|| helper())();
    a();
    b(1);
}

pub async fn asynchronous() {
    async { helper() }.await;
    helper();
}

pub fn nested_items() {
    fn inner() {
        helper();
    }
    const N: u8 = helper_const();
    struct Local;
    impl Local {
        fn method(&self) {
            helper();
        }
    }
    inner();
}

pub const FROM_CONST: u8 = helper_const();
pub static FROM_STATIC: u8 = helper_const();

pub trait Declared {
    fn declared(&self);
    fn defaulted(&self) {
        self.declared();
        helper();
    }
}

/// Operators, `?`, `for` and `Drop` are calls to the compiler and not sites; `Some(v)` is a call.
pub fn operators(a: u8, o: Option<u8>) -> Option<u8> {
    let _sum = a + 1;
    let v = o?;
    for _ in 0..v {}
    Some(v)
}

pub fn order(r: &Rec) -> u8 {
    twice(r.value()) + helper()
}

/// Recursion is shown, not entered again.
pub fn recurse_a() {
    recurse_b();
}

pub fn recurse_b() {
    recurse_a();
}

/// The same def called at two sites: two nodes in the unmerged wire, one with `call_count` 1 in the merged one.
pub fn calls_helper_twice() {
    helper();
    twice(1);
    helper();
}

/// A chain of concrete calls, five deep: a tree with a node at depth 4, so a budget of depth 3 cuts something (the stitching test of `tests/session.rs`).
pub fn chain_a() {
    chain_b();
}

pub fn chain_b() {
    chain_c();
}

pub fn chain_c() {
    chain_d();
}

pub fn chain_d() {
    chain_e();
}

pub fn chain_e() {}
