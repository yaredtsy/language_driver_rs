// B11: a const or static and a type-namespace def spelled the same are legal Rust, and what each holds is named `C::helper`. `const _` adds
// nothing, so its items are named as the enclosing def's own. Every repeated qualname is told apart by `occurrence`.
const C: u8 = { fn helper() {} 1 };
mod C { fn helper() {} }

static X: u8 = { fn helper() {} 2 };
trait X { fn helper() {} }

const S: u8 = { fn helper() {} 3 };
struct S {}
impl S { fn helper() {} }

const _: () = { fn g() {} };
fn g() {}

mod m {
    const _: () = { fn f() {} };
    fn f() {}
}

// after a function the joiner is `{fn}`, which is not an identifier: no collision with a type
fn T() { fn helper() {} }
struct T {}
impl T { fn helper() {} }
