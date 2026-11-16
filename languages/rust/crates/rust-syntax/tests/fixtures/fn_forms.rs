fn plain() {}
pub fn public(x: u8, (y, z): (u8, u8)) -> u8 { x }
pub(crate) async fn restricted_async() {}
const fn constant() {}
unsafe fn dangerous() {}
extern "C" fn c_abi() {}
fn generic<T: Clone, const N: usize>(t: T, arr: [u8; N]) {}
fn with_where<T>(t: T) where T: Copy {}
