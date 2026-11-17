/// ID: marked-struct
pub struct MarkedStruct {
    pub a: u8,
}

pub struct Tuple(pub u8, pub u8);

pub enum Shape {
    Unit,
    Pair(u8, u8),
    Named { a: u8 },
}

macro_rules! make_tuple {
    ($name:ident) => {
        pub struct $name(pub u8);
    };
}

make_tuple!(Made);

pub fn build_marked() -> MarkedStruct {
    MarkedStruct { a: 1 }
}

pub fn build_tuple() -> Tuple {
    Tuple(1, 2)
}

/// A variant is built as its enum: `Shape` by name, and a unit variant named as a value is no construction at all.
pub fn build_variants() {
    Shape::Pair(1, 2);
    Shape::Named { a: 1 };
    let _unit = Shape::Unit;
}

/// An external variant keeps its name: `Some`, `Ok` and `Err` are three targets, and `None` is a unit variant: no site.
pub fn build_external() {
    Some(1);
    Ok::<u8, u8>(2);
    Err::<u8, u8>(3);
    let _none: Option<u8> = None;
    std::ops::Range { start: 0, end: 1 };
}

/// A struct a macro made has no outline def: external, named, and not counted as a body that was not walked (it has none).
pub fn build_generated() {
    Made(1);
}
