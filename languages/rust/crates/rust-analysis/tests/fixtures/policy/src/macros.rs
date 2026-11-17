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
