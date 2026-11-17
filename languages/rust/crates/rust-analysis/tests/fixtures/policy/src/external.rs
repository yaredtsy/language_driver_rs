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
