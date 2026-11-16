// I10: an impl whose header cannot be read lists nothing; the impls and functions around it are listed.
fn before() {}
impl for S { fn lost() {} }
impl Tr for { fn lost_too() {} }
impl { fn lost_three() {} }
impl S { fn kept() {} }
fn after() {}
// An unclosed generic list swallows what follows it, so this case goes last.
impl S<u8 { fn lost_four() {} }
