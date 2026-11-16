/// A struct.
pub struct Plain;
struct Tuple(u8, u8);
struct Record { a: u8 }
union Bits { a: u8, b: i8 }
enum Choice { A, B(u8), C { n: u8 } }
trait Speak {
    fn speak(&self);
}
type Alias = Vec<u8>;
const LIMIT: usize = 3;
static GREETING: &str = "hi";
macro_rules! shout { () => {}; }
#[macro_export]
macro_rules! exported { () => {}; }
const _: () = ();
use std::fmt;
extern crate core;
mod declared;
