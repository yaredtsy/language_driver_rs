// I4, I6, I12: a path is shortened to its last segment, the arguments stay whole; an impl in a module takes the module's path.
impl a::b::S { fn m() {} }
impl a::S<b::T> { fn m() {} }
impl fmt::Display for a::b::S { fn fmt() {} }
impl From<io::Error> for a::S<b::T> { fn from() {} }
impl ::std::fmt::Debug for ::a::S { fn fmt() {} }
impl crate::Tr for self::S { fn m() {} }
impl ::S { fn m() {} }

mod a {
    mod b {
        impl S { fn m() {} }
        impl T for S { fn m() {} }
        impl<T> From<T> for S<T> { fn from() {} }
    }
    impl S { fn n() {} }
}
