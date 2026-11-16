include!(concat!(env!("OUT_DIR"), "/generated.rs"));

pub fn run() {
    generated();
}
