// P1 and P7: one space where the rule puts one, nowhere else; line breaks and comments change nothing.
impl T<S< u8 >> for S { fn m() {} }
impl T<S</* why */ u8>> for S { fn m() {} }
impl<'a> T<&'a mut [u8 ;4]> for S { fn m() {} }
impl T<Box<dyn Fn(u8)->u8+Send+'static>> for S { fn m() {} }
impl T<dyn Iterator<Item=u8>> for S { fn m() {} }
impl T<dyn Iterator<Item:Display>> for S { fn m() {} }
impl T<<X as Tr>::Out> for S { fn m() {} }
impl T<fn(a:u8)> for S { fn m() {} }
impl T<fn(a: u8, b: &str) -> bool> for S { fn m() {} }
impl T<*const S> for S { fn m() {} }
impl T<*mut dyn Tr> for S { fn m() {} }
impl T<&dyn Tr> for S { fn m() {} }
impl T<&mut dyn Tr> for S { fn m() {} }
impl T<unsafe extern "C" fn(i32)> for S { fn m() {} }
impl T<extern fn()> for S { fn m() {} }
impl T<a :: B> for S { fn m() {} }
impl T<dyn Tr+?Sized> for S { fn m() {} }
impl T<(A , B)> for
    S
{ fn m() {} }
impl T<
    Vec<
        // a comment
        u8,
    >,
> for S { fn m() {} }
