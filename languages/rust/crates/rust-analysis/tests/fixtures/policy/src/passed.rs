use crate::shapes::Unit;

fn parse(s: &str) -> u8 {
    s.len() as u8
}

fn worker() {}

fn spawn(f: fn()) {
    f()
}

pub trait Build {
    fn build(x: u8) -> Self;
}

pub struct Wrapper(pub u8);

pub enum E {
    P(u8),
    U,
}

pub struct Parser;

impl Parser {
    fn helper(x: u8) -> u8 {
        x
    }

    /// A function in a method path: by `Self` and by the type's name.
    pub fn run(xs: Vec<u8>) -> usize {
        xs.iter().copied().map(Self::helper).count() + xs.into_iter().map(Parser::helper).count()
    }

    pub fn as_value() {
        let f = Self::helper;
        f(1);
    }
}

/// Named, in a value position.
pub fn named(xs: Vec<&str>) -> usize {
    xs.into_iter().map(parse).count()
}

pub fn handed_to_a_function() {
    spawn(worker);
}

/// A def both called and passed in one body has both edges, at their own sites.
pub fn called_and_passed() {
    worker();
    spawn(worker);
}

/// A constructor handed over: no edge, counted. A unit struct and a unit variant named as values are no site.
pub fn constructors(xs: Vec<u8>) {
    xs.iter().copied().map(Some).count();
    let _f = E::P;
    xs.into_iter().map(Wrapper).count();
    let _unit = Unit;
    let _unit_variant = E::U;
}

impl Wrapper {
    pub fn all(xs: Vec<u8>) -> usize {
        xs.into_iter().map(Self).count()
    }
}

/// A function of another crate handed over: no edge, counted.
pub fn externals(xs: Vec<&str>) -> usize {
    xs.into_iter().map(String::from).count()
}

/// A local that holds a function is a variable read: no site. Calling it is a call through a value of function type.
pub fn locals(f: fn(u8) -> u8, x: u8) -> u8 {
    let g = f;
    let c = || x;
    c();
    g(x)
}

/// A path through a type parameter is a hand-over whose answer is open.
pub fn through_a_parameter<W: Build>(xs: Vec<u8>) -> usize {
    xs.into_iter().map(W::build).count()
}
