// Spacing, line breaks, comments, a where clause and the impl's own generics never rename a def: both impls are <Wrapper<T> as From<&[T]>>::from.
impl<'a, T: Clone> From<&'a [T]> for Wrapper<'a, T> where T: Copy { fn from() {} }

impl <'x,T:Clone+Copy>   From  <  & 'x  /* why */ [T] , >   for  Wrapper
    < 'x , T >
    where T : Send
{ fn from() {} }
