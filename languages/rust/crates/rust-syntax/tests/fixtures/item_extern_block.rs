unsafe extern "C" {
    /// Declared elsewhere.
    pub fn abs(x: i32) -> i32;
    safe fn getpid() -> i32;
    static ERRNO: i32;
    type Opaque;
}
extern "C" {
    fn legacy();
}
mod ffi {
    extern "C" {
        fn inside_mod(p: *const u8);
    }
}
