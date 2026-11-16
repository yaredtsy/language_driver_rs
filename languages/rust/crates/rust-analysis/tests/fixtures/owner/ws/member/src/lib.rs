macro_rules! make {
    ($name:ident) => {
        pub fn $name() {}
    };
}

make!(generated);

pub fn in_member() {}
