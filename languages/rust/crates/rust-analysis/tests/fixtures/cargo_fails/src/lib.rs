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

pub fn uses_std() {
    let s = String::new();
    s.len();
    Some(1);
    println!("{}", answer());
}

fn answer() -> u8 {
    1
}
