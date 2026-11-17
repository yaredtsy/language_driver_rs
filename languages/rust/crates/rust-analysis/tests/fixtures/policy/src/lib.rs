//! The policy workspace (RS4 §8): every kind of def the outline lists, in more than one file, with the shapes the join test treats as exceptions
//! (step 1), and `calls`, `macros` and `cfg_off`, whose site lists are the snapshots of step 2.
#![allow(dead_code, unused)]

mod bom;
mod calls;
mod cfg_off;
mod macros;
mod shapes;
pub mod nested;

pub trait Writer {
    fn write(&self) -> u8;
    fn twice(&self) -> u8 {
        self.write() + self.write()
    }
}

pub trait Make {
    const KIND: u8;
    fn make() -> Self;
}

pub struct Json;

impl Writer for Json {
    fn write(&self) -> u8 {
        1
    }
}

impl Make for Json {
    const KIND: u8 = 7;
    fn make() -> Self {
        Json
    }
}

pub fn plain() -> u8 {
    helper() + 1
}

fn helper() -> u8 {
    2
}

/// A def written inside a body is named after it (`with_inner::{fn}::inner`) and is a def of its own.
pub fn with_inner() {
    fn inner() {}
    inner();
}

/// ID: marked-one
pub fn marked() {}

/// ID: twin
pub fn twin_a() {}

/// ID: twin
pub fn twin_b() {}

/// The engine has no def for what a `cfg` turns off, and an entry there is `None`: the join test's listed exception.
#[cfg(any())]
pub fn disabled() {
    helper();
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test() {}
}

macro_rules! make_fn {
    ($name:ident) => {
        pub fn $name() {
            helper();
        }
    };
}

make_fn!(generated);

pub fn calls_generated() {
    generated();
}
