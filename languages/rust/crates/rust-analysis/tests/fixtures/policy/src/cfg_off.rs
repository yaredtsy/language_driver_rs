fn f() -> u8 {
    1
}

pub fn statement() {
    #[cfg(any())]
    f();
    f();
}

pub fn block() {
    #[cfg(any())]
    {
        f();
    }
    f();
}

pub fn let_statement() {
    #[cfg(any())]
    let _a = f();
}

pub fn match_arm(x: u8) -> u8 {
    match x {
        #[cfg(any())]
        0 => f(),
        _ => 2,
    }
}

pub fn macro_statement() {
    #[cfg(any())]
    println!("{}", f());
    f();
}

pub fn method_statement() {
    let v = vec![1u8];
    #[cfg(any())]
    v.len();
    v.len();
}

pub fn closure_statement() {
    #[cfg(any())]
    let _c = || f();
}

pub fn on_statement() {
    #[cfg(not(any()))]
    f();
}

pub fn struct_expression() {
    #[cfg(any())]
    let _r = crate::shapes::Rec { a: f() };
    let _s = crate::shapes::Rec { a: 1 };
}
