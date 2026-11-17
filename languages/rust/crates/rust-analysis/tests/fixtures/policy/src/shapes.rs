pub struct Unit;
pub struct Pair(pub u8, pub u8);
pub struct Rec {
    pub a: u8,
}
pub union Bits {
    pub whole: u32,
    pub half: u16,
}
pub enum E {
    V(u8),
    R { a: u8 },
    U,
}

pub const LIMIT: u8 = limit();
const fn limit() -> u8 {
    3
}
pub static NAME: &str = "policy";

impl Rec {
    pub const K: u8 = 1;
    pub fn mk() -> Self {
        Self { a: 1 }
    }
    pub fn get(&self) -> u8 {
        self.a
    }
}

// a second block of the same type: its `get` is the second `Rec::get` of this file (D15, D18)
impl Rec {
    pub fn get(&self) -> u8 {
        self.a + 1
    }
}

pub type Alias = Rec;

#[derive(Default, Clone)]
pub struct Derived {
    pub n: u8,
}

pub fn derived() -> (Derived, Derived) {
    (Derived::default(), Derived { n: 1 }.clone())
}

unsafe extern "C" {
    pub safe fn foreign(x: u8) -> u8;
}
