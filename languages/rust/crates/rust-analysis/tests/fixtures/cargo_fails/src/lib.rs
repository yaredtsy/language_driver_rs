pub struct Thing;

impl Thing {
    pub fn new() -> Thing {
        Thing
    }

    pub fn run(&self) {}
}

pub fn go() {
    Thing::new().run();
}
