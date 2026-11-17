pub fn run() -> bool {
    let n = shop_core::helper();
    let _ = n;
    shop_core::Order::new().submit()
}
