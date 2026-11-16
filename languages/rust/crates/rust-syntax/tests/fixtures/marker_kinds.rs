// §3 for every kind of def: the marker is the def's own, and an `impl` block, which is no def, gives its doc line to nobody.
/// ID: function
fn function() {}

/// ID: structure
struct Structure;

/// ID: enumeration
enum Enumeration {}

/// ID: union_def
union UnionDef { a: u8 }

/// ID: type_alias
type Alias = u8;

/// ID: constant
const CONSTANT: u8 = 1;

/// ID: static_item
static STATIC_ITEM: u8 = 1;

/// ID: macro_def
#[macro_export]
macro_rules! macro_def { () => {}; }

/// ID: module
mod module {
    /// ID: in_module
    fn in_module() {}
}

/// ID: tr
trait Tr {
    /// ID: required
    fn required();
    /// ID: provided
    fn provided() {}
    /// ID: assoc_const
    const C: u8;
}

/// ID: on_the_impl
impl Structure {
    /// ID: inherent_method
    fn inherent() {}
    /// ID: inherent_const
    const C: u8 = 1;
}

impl Tr for Structure {
    /// ID: trait_impl_method
    fn required() {}
}

unsafe extern "C" {
    /// ID: foreign
    fn foreign();
}

fn outer() {
    /// ID: local
    fn local() {}
    /// ID: local_struct
    struct Local;
}
