#![allow(dead_code, unused)]
use std::ops::Add;

pub trait Writer {
    fn write(&self) -> u8;
    fn step(&self) -> u8;
    fn twice(&self) -> u8 {
        self.step() + self.step()
    }
}
pub trait Make {
    fn make() -> Self;
}
pub struct JsonWriter;
impl Writer for JsonWriter {
    fn write(&self) -> u8 { 1 }
    fn step(&self) -> u8 { 2 }
}
impl Make for JsonWriter {
    fn make() -> Self { JsonWriter }
}
#[derive(Default)]
pub struct Foo;
impl Foo {
    pub fn new() -> Foo { Foo }
    pub fn helper(x: u8) -> u8 { x }
    pub fn run(&self, xs: Vec<u8>) -> Vec<u8> {
        xs.into_iter().map(Self::helper).collect()
    }
}
pub struct Pair(pub u8, pub u8);
impl Pair {
    pub fn mk() -> Self { Self(1, 2) }
}
pub enum E {
    V(u8),
    R { a: u8 },
}
pub struct Rec { pub a: u8 }
impl Rec {
    pub fn mk() -> Self { Self { a: 1 } }
}
pub struct Wrapper(pub u8);
pub struct H { pub cb: fn(u8) -> u8 }
pub struct P(pub u8);
impl Add for P {
    type Output = P;
    fn add(self, o: P) -> P { P(self.0 + o.0) }
}

fn parse(s: &str) -> u8 { 0 }
fn worker() {}
fn spawn(f: fn()) { f() }
fn inner_call() -> u8 { 3 }
fn f() -> u8 { 1 }
fn g() -> u8 { 2 }
fn helper() {}
async fn work() {}
fn make() -> impl Writer { JsonWriter }

// ---- the cases, one function each ----
fn c01_concrete_trait_impl(json: JsonWriter) { json.write(); }
fn c02a_generic_bound<W: Writer>(w: &W) { w.write(); }
fn c02b_generic_by_value<W: Writer>(w: W) { w.write(); }
fn c03a_dyn_ref(w: &dyn Writer) { w.write(); }
fn c03b_box_dyn(w: Box<dyn Writer>) { w.write(); }
fn c04a_impl_arg(w: impl Writer) { w.write(); }
fn c04b_impl_return() { make().write(); }
fn c05_trait_default_method_is_above_in_Writer_twice() {}
fn c06a_inherent_assoc() { Foo::new(); }
fn c06b_generic_assoc<W: Make>() { W::make(); }
fn c06c_qualified_default() { <Foo as Default>::default(); }
fn c06d_inferred_default() { let f: Foo = Default::default(); }
pub struct Bar;
impl Default for Bar { fn default() -> Bar { Bar } }
fn c06f_handwritten_default() { <Bar as Default>::default(); let b: Bar = Default::default(); }
fn c06e_trait_path_call() { <JsonWriter as Make>::make(); }
fn c07a_tuple_struct() { Pair(1, 2); }
fn c07b_tuple_variant() { E::V(1); }
fn c07c_record_struct() { Rec { a: 1 }; }
fn c07d_record_variant() { E::R { a: 1 }; }
fn c07e_self_record() { Rec::mk(); }
fn c07f_self_tuple() { Pair::mk(); }
fn c08a_closure_local() { let f = |x: u8| x; f(1); }
fn c08b_field_fn_ptr(h: &H) { (h.cb)(1); }
fn c08c_fn_ptr_param(f: fn()) { f(); }
fn c08d_fn_trait_bound<F: Fn()>(f: F) { f(); }
fn c08e_dyn_fn(f: &dyn Fn(u8) -> u8) { f(1); }
fn c09a_fn_as_value(xs: Vec<&str>) { xs.iter().map(|s| parse(s)).count(); xs.into_iter().map(parse).count(); }
fn c09b_spawn() { spawn(worker); }
fn c09c_assoc_as_value(xs: Vec<u8>) { xs.into_iter().map(Foo::helper).count(); xs.into_iter().map(Wrapper).count(); }
fn c10a_println() { println!("{}", f()); }
fn c10b_vec() { let v = vec![g()]; }
macro_rules! body_call { () => { inner_call() }; }
fn c11a_macro_rules_body() { body_call!(); }
macro_rules! gen_fn { ($n:ident) => { fn $n() { helper() } }; }
gen_fn!(generated);
fn c11b_calls_generated() { generated(); }
#[tokio::main]
async fn c12_tokio_main() { work().await; }
fn c13_operators(a: P, b: P, x: Option<u8>, p: &u8, it: Vec<u8>) -> Option<u8> {
    let s = a + b;
    let v = x?;
    let d = *p;
    for y in it { }
    Some(v)
}
async fn c13b_await() { work().await; }
