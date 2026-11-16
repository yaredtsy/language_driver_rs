mod outer {
    //! the module's own docs
    pub fn f() {}
    pub struct S;
    mod inner {
        pub fn f() {}
        const C: u8 = 1;
        macro_rules! m { () => {}; }
    }
    pub mod declared_inside;
    trait T {}
}
mod empty {}
#[cfg(test)]
mod tests {
    fn helper() {}
}
fn after() {}
