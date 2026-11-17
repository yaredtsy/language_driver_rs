pub fn std_calls() {
    let mut s = String::new();
    s.push('a');
    s.len();
    let _v: Vec<u8> = Vec::new();
    Some(1);
    Ok::<u8, ()>(1);
    std::ops::Range { start: 0, end: 1 };
    let _d: u8 = Default::default();
    crate::shapes::derived();
}

/// One external of each row of RS4 §5's naming table that `std_calls` does not have: a trait impl with an argument, a slice, a free function.
pub fn std_naming(mut a: u8, mut b: u8, v: &[u8]) {
    String::from("x");
    v.len();
    std::mem::swap(&mut a, &mut b);
}
