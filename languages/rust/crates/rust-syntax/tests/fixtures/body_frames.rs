// B5: a closure, an async block and every kind of block are no naming frame: the item is named as if they were not there.
fn outer() {
    let closure = || { fn in_closure() {} };
    let nested = || { let inner = || { struct DeepClosure; }; };
    let a = async { fn in_async() {} };
    let b = async move { enum InAsyncMove { A } };
    if true { fn in_if() {} } else { fn in_else() {} }
    loop { fn in_loop() {} break; }
    for _ in 0..1 { fn in_for() {} }
    match 1 { _ => { fn in_arm() {} } }
    unsafe { fn in_unsafe() {} }
    'label: { fn in_labelled() {} }
    let c = const { fn in_const_block() {} 1 };
    { { fn in_plain_block() {} } }
}
