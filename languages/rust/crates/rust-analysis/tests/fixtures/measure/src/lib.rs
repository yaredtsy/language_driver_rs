pub struct Item;

impl Item {
    pub fn run(&self) {}
}

/// `run` is reached through `Option::unwrap`.
pub fn through_unwrap(item: Option<Item>) {
    item.unwrap().run();
}

/// `run` is reached through `Vec::iter`.
pub fn through_iter(items: Vec<Item>) {
    for item in items.iter() {
        item.run();
    }
}
