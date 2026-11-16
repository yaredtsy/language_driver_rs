// I8: a self type that is not a path is printed whole; angle brackets only when the impl is inherent.
impl<T> [T] { fn m() {} }
impl dyn Tr { fn m() {} }
impl dyn Tr + Send { fn m() {} }
impl<'a> dyn Tr + 'a { fn m() {} }
impl &S { fn m() {} }
impl T for &S { fn m() {} }
impl<'a> T for &'a mut S { fn m() {} }
impl T for (A, B) { fn m() {} }
impl T for () { fn m() {} }
impl T for (A,) { fn m() {} }
impl T for (S) { fn m() {} }
impl T for fn() { fn m() {} }
impl T for fn(u8) -> u8 { fn m() {} }
impl T for *const S { fn m() {} }
impl T for [u8; 4] { fn m() {} }
impl T for ! { fn m() {} }
impl T for <A as B>::C { fn m() {} }
impl T for m!() { fn m() {} }
impl Tr for dyn Other { fn m() {} }
