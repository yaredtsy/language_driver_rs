// I9: a blanket impl names its self type by the parameter's name.
impl<T: Display> Tr for T { fn m(&self) {} }
impl<T> Tr for &T { fn m(&self) {} }
impl<T: ?Sized> Tr for Box<T> { fn m(&self) {} }
