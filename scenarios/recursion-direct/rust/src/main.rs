fn helper() -> u8 {
    1
}

fn countdown(n: u8) {
    if n > 0 {
        countdown(n - 1);
    }
    helper();
}

fn main() {
    countdown(3);
}
