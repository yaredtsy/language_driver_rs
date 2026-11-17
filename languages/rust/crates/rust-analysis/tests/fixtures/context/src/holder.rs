use crate::{Json, Writer};

/// A struct holding a `Box<dyn Writer>`: what `run` calls depends on what was put in, which is another function's business (`dyn`).
pub struct Holder {
    pub writer: Box<dyn Writer>,
}

impl Holder {
    pub fn new(writer: Box<dyn Writer>) -> Holder {
        Holder { writer }
    }

    pub fn run(&self) -> u8 {
        self.writer.write()
    }
}

pub fn use_holder() -> u8 {
    Holder::new(Box::new(Json)).run()
}
