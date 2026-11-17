/// A callback: a `fn` pointer (`fn_pointer`), a parameter bounded by `Fn` (`fn_param`), and a closure handed to it. The def handed over is a passed edge.
pub fn apply(f: fn(u8) -> u8, x: u8) -> u8 {
    f(x)
}

pub fn apply_generic<F: Fn(u8) -> u8>(f: F, x: u8) -> u8 {
    f(x)
}

fn double(x: u8) -> u8 {
    x * 2
}

pub fn use_callbacks() -> u8 {
    apply(double, 1) + apply_generic(double, 2) + apply_generic(|x| x + 1, 3)
}
