// I11: two impls that spell one type have one qualname (occurrence tells them apart in step 5). S<u8> and S<u16> are not twins.
impl S { fn m() {} }
impl S { fn m() {} }
#[cfg(unix)]
impl S { fn m() {} }
impl S<u8> { fn m() {} }
impl S<u16> { fn m() {} }
