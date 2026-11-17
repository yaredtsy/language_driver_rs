use crate::calls::Holder;
use crate::{Json, Make, Writer};

pub trait Step {
    fn step(&self) -> u8;
    fn twice(&self) -> u8 {
        self.step() + self.step()
    }
    fn fresh() -> Self
    where
        Self: Sized;
    fn pair() -> (Self, Self)
    where
        Self: Sized,
    {
        (Self::fresh(), Self::fresh())
    }
}

fn make_writer() -> impl Writer {
    Json
}

fn make_generic<W: Writer + Default>() -> impl Writer {
    W::default()
}

pub fn concrete(json: Json) {
    json.write();
    json.twice();
}

pub fn generic_by_reference<W: Writer>(w: &W) {
    w.write();
}

pub fn generic_by_value<W: Writer>(w: W) {
    w.write();
}

pub fn dyn_reference(w: &dyn Writer) {
    w.write();
}

pub fn boxed_dyn(w: Box<dyn Writer>) {
    w.write();
}

pub fn impl_argument(w: impl Writer) {
    w.write();
}

/// The engine sees through a return-position `impl Trait` to the hidden type: the impl's method.
pub fn impl_return() {
    make_writer().write();
}

pub fn impl_return_of_a_parameter<W: Writer + Default>() {
    make_generic::<W>().write();
}

pub fn static_through_a_parameter<W: Make>() {
    W::make();
}

pub fn qualified_concrete() {
    <Json as Make>::make();
}

pub fn qualified_through_a_parameter<W: Make>() {
    <W as Make>::make();
}

pub fn closure_value() {
    let f = |x: u8| x;
    f(1);
}

pub fn fn_pointer(f: fn(u8) -> u8) {
    f(1);
}

pub fn fn_pointer_field(h: &Holder) {
    (h.cb)(1);
}

pub fn fn_bound<F: Fn(u8) -> u8>(f: F) {
    f(1);
}

pub fn dyn_fn(f: &dyn Fn(u8) -> u8) {
    f(1);
}

pub fn unknown_callee() {
    missing()(1);
}

pub fn unknown_method(x: Undefined) {
    x.nothing();
}

/// The receiver's type is a projection: not concrete, and none of the kinds `Open` names.
pub fn projection_receiver<I: Iterator>(mut items: I)
where
    I::Item: Writer,
{
    items.next().unwrap().write();
}

/// A trait-qualified path says nothing about the type: it is the substitution's `Self`.
pub fn trait_path_through_a_parameter<W: Writer>(w: &W) {
    Writer::write(w);
}

pub fn trait_path_on_a_concrete_type(json: Json) {
    Writer::write(&json);
}

pub fn trait_path_through_an_iterator<I: Iterator>(mut it: I) {
    Iterator::next(&mut it);
}

pub fn default_through_a_parameter<W: Default>() {
    let _x: W = Default::default();
}

pub fn default_on_a_concrete_type() {
    let _x: u8 = Default::default();
}
