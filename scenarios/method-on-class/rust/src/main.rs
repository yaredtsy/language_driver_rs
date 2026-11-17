struct Counter;

impl Counter {
    fn bump(&self) -> u8 {
        self.read()
    }

    fn read(&self) -> u8 {
        1
    }

    fn reset() -> u8 {
        0
    }
}

fn main() {
    let counter = Counter;
    counter.bump();
    Counter::reset();
}
