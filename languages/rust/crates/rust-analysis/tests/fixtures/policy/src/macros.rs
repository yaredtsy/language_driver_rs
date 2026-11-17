fn f() -> u8 {
    1
}

fn g() -> u8 {
    2
}

fn emit() {}

macro_rules! log {
    () => {
        emit()
    };
    ($e:expr) => {
        println!("{}", $e)
    };
}

macro_rules! both {
    ($e:expr) => {
        ($e, $e)
    };
}

macro_rules! nested {
    () => {
        log!()
    };
}

macro_rules! statements {
    () => {
        emit();
        emit();
    };
}

pub fn std_arguments() {
    println!("{}", f());
    let _v = vec![g()];
    assert!(f() > 0);
}

pub fn project_body() {
    log!();
}

/// `f()` is written where it is; `println!`'s own call inside the project macro's body is an external macro's, never a site.
pub fn project_wrapping_external() {
    log!(f());
}

/// `log!()` is written in `nested`'s body, so its own body's call is at the outermost call written in the file.
pub fn macro_in_macro() {
    nested!();
}

pub fn argument_twice() {
    both!(f());
}

pub fn statements_in_a_body() {
    statements!();
    f();
}

pub fn not_expanded() {
    unknown_macro!(f());
    g();
}

macro_rules! wrap {
    () => {
        vec![g()]
    };
}

macro_rules! wrap_argument {
    ($e:expr) => {
        vec![$e]
    };
}

macro_rules! nest_wrap {
    () => {
        wrap!()
    };
}

macro_rules! make_function {
    ($name:ident) => {
        pub fn $name() {}
    };
}

make_function!(by_macro);

/// `g()` is written in the body of a project macro, as an argument of an external one: the text of `wrap`, not of `vec!`, so a site at `wrap!`.
pub fn project_macro_around_external() {
    let _v = wrap!();
}

/// The same call written by the user, in the arguments of a project macro that hands it to `vec!`: a call where it is written.
pub fn argument_through_external() {
    let _v = wrap_argument!(g());
}

/// Two project levels and an external one: the macro whose text it is is `wrap`, written in `nest_wrap`'s body: the outermost call written in the file.
pub fn two_project_levels() {
    let _v = nest_wrap!();
}

/// Arguments of a macro the engine cannot expand give no site, and the macro is counted once, not once for the call inside and once for it.
pub fn unknown_around_a_call() {
    unknown_macro!(f());
}

pub fn unknown_in_unknown() {
    unknown_macro!(other_unknown!(f()));
    g();
}

/// An unknown macro in the arguments of one that expands: the expansion holds it, it cannot be expanded, and it is counted once.
pub fn unknown_in_known() {
    let _v = vec![unknown_macro!(f())];
}

#[derive(Default, Clone)]
pub struct Plain;

/// A built-in derive's methods have no source of their own: `External`, named by the type, counted `macro_generated_target`.
pub fn derived_methods() {
    let plain = Plain::default();
    let _copy = plain.clone();
}

/// A function a project macro made is `External` and counted `macro_generated_target` once per call.
pub fn calls_made_functions() {
    by_macro();
    by_macro();
}

macro_rules! lost_inside {
    () => {
        unknown_macro!()
    };
}

/// A macro that repeats its argument meets the unknown call twice: one written call, one count (a repeated argument is one site, too).
pub fn unknown_in_a_repeated_argument() {
    let _pair = both!(unknown_macro!(f()));
}

/// An unknown macro in a project macro's body has no place in the file: each macro call that holds it is its own loss, so two calls count two.
pub fn unknown_in_a_project_macros_body() {
    lost_inside!();
    lost_inside!();
}
