// A free function can carry a receiver: it parses, and `rustc` rejects it later. The outline reads the parse.
fn by_value(self) {}
fn by_mut_value(mut self) {}
fn by_ref(&self) {}
fn by_mut_ref(&mut self) {}
fn by_ref_lifetime<'a>(&'a self, n: u8) {}
fn by_mut_ref_lifetime<'a>(&'a mut self) {}
fn typed(self: Box<Self>) {}
fn typed_mut(mut self: Rc<Self>, n: u8) {}
fn typed_ref(self: &Self) {}
