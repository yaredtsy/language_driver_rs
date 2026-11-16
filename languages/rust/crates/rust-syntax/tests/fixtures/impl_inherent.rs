// I1 to I3 and D13, D16: an inherent impl names its items by the self type; the impl itself is no def.
struct S;
struct G<T>(T);
struct P<'a>(&'a str);
struct W<'a, T>(&'a T);

/// The docs of an impl block belong to no def.
impl S {
    const LIMIT: u8 = 1;
    pub fn new() -> S { S }
    /// A method with a receiver.
    pub fn get(&self, n: u8) {}
}

impl<T> G<T> {
    fn m(&self) {}
}
impl G<u8> {
    fn m(&self) {}
}
impl G<u16> {
    fn m(&self) {}
}

impl<'a> P<'a> {
    fn m(&self) {}
}
impl<'a, T> W<'a, T> {
    fn m(&self) {}
}
