// I5, D13, D14: a trait impl is <Self as Trait>::item. An associated type is no def. The impl's generics and keywords are not in the name.
struct S;
trait T { fn m(&self); const C: u8; type Out; }

impl T for S {
    const C: u8 = 1;
    type Out = u8;
    fn m(&self) {}
}
impl<T: Clone> Clone for W<T> where T: Copy {
    fn clone(&self) -> Self { loop {} }
}
impl<'a> T for P<'a> { fn m(&self) {} const C: u8 = 2; type Out = (); }
unsafe impl UnsafeT for S { unsafe fn danger(&self) {} }
unsafe impl Send for S {}
impl !Sync for S {}
impl T for S { m!(); }
