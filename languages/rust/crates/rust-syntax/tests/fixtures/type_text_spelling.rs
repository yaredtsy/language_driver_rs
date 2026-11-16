// T1 to T5: a type is named the way it is written. Nothing is looked up, expanded or shortened.
type Bytes = Vec<u8>;
impl From<Bytes> for S { fn from() {} }
impl From<Vec<u8>> for S { fn from() {} }
impl From<io::Error> for S { fn from() {} }
impl From<std::io::Error> for S { fn from() {} }
impl From<Error> for S { fn from() {} }
impl Add<Self> for S { fn add() {} }
impl<T> From<T> for S<T> { fn from() {} }
impl<U> From<U> for S<U> { fn from() {} }
impl From<::std::io::Error> for S { fn from() {} }
