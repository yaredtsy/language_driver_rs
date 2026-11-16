// D4, D11, D13, D14: the items of a trait are Methods named under the trait, with or without a body. The trait's generics are not in its name.
trait Speak {
    const LOUD: bool;
    const DEFAULT: u8 = 1;
    type Out;
    fn speak(&self);
    fn shout(&self) { self.speak() }
    fn make() -> Self;
    m!();
}
trait Generic<A, B = u8> { fn m(&self, a: A); }
trait Lifetimed<'a> { fn m(&self) -> &'a str; }
unsafe trait Danger { unsafe fn run(&self); }
trait Empty {}
mod audio {
    trait Mixer { fn mix(&self); }
}
