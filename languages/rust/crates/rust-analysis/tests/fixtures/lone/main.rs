mod child;

pub fn run() {
    child::go();
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
