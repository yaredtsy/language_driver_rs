// I7: the trait's generic arguments are kept, so two From impls of one type have two names.
impl From<A> for S { fn from(a: A) -> S { S } }
impl From<B> for S { fn from(b: B) -> S { S } }
impl From<io::Error> for a::S<b::T> { fn from(e: io::Error) -> Self { loop {} } }
impl<T> From<T> for S<T> { fn from(t: T) -> Self { loop {} } }
impl<'a, T: Clone> From<&'a [T]> for Wrapper<'a, T> { fn from(s: &'a [T]) -> Self { loop {} } }
impl Add<Self> for S { fn add(self, other: Self) -> S { S } }
impl<T, const N: usize> Tr<[T; N], 3> for S { fn m() {} }
