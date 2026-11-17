/// A struct with a marker. Rust has no constructor function, so building it is a leaf.
///
/// ID: 11111111-0000-0000-0000-000000000001
struct Report {}

/// ID: 11111111-0000-0000-0000-000000000002
struct Plain {}

fn main() {
    Report {};
    Plain {};
}
