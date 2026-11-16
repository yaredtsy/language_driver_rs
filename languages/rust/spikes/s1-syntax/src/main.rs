use ra_ap_syntax::{Edition, SourceFile};
fn main() {
    let p = SourceFile::parse("fn main() {}", Edition::CURRENT);
    println!("{} errors", p.errors().len());
}
