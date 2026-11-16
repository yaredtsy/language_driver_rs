// P2, P3: lifetimes are dropped with the , or + that joined them; empty angle brackets go; parentheses never do.
impl<'a> T<Cow<'a, str>> for S { fn m() {} }
impl<'a> T<Pair<X, 'a>> for S { fn m() {} }
impl<'a> T<P<'a>> for S { fn m() {} }
impl T<P<'_>> for S { fn m() {} }
impl<'a, 'b> T<Q<'a, 'b>> for S { fn m() {} }
impl<'a> T<&'a mut X> for S { fn m() {} }
impl T<&'static str> for S { fn m() {} }
impl T<Box<dyn Tr + 'static>> for S { fn m() {} }
impl<'a> T<Box<dyn 'a + Tr + Send>> for S { fn m() {} }
impl T<for<'a> fn(&'a str) -> &'a str> for S { fn m() {} }
impl T<Box<dyn for<'a> Fn(&'a u8)>> for S { fn m() {} }
impl<'a> T<dyn Iterator<Item = &'a u8>> for S { fn m() {} }
impl<'a> T for &'a S { fn m() {} }
impl T for dyn Tr + 'static { fn m() {} }
impl T<E<>> for S { fn m() {} }
impl T<()> for S { fn m() {} }
impl T<fn()> for S { fn m() {} }
impl T<Box<dyn Fn()>> for S { fn m() {} }
impl T<(Box<dyn FnMut()>, ())> for S { fn m() {} }
