// I10 in a body (B2): a local impl whose header cannot be read lists nothing, and the function that holds the error is flagged.
fn before() {}
fn h() {
    impl S<u8 { fn lost() {} }
    fn after() {}
}
