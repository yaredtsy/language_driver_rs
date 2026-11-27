//! What the context layer may know (`docs/03-typescript/12-context-layer/03-design.md` §15.2). Private: users see `Precision`, which maps to a set
//! (`Precision::layer`). A test may run the layer with fewer bits, to show that a phase changes nothing with its bit off.

bitflags::bitflags! {
    /// One bit per kind of knowledge, one phase each. A bit does something only once its phase is built (`Caps::BUILT`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct Caps: u8 {
        /// TC2: functions through parameters.
        const PARAMS = 1;
        /// TC3: variables of a body.
        const LOCALS = 2;
        /// TC3: what a nested function reads from the functions around it.
        const CAPTURES = 4;
        /// TC4: objects and their properties.
        const OBJECTS = 8;
        /// TC5: instances and their methods.
        const INSTANCES = 16;
        /// TC6: return values.
        const RETURNS = 32;
        /// TC3 step 4: a variable at the top of a file, read from a function or through an import, has its value.
        const MODULES = 64;
        /// TC3 step 6: a parameter that cannot reach a call is not in the key.
        const RELEVANCE = 128;
    }
}

impl Caps {
    /// The bits that are built: what `Precision::Precise` turns on. `PARAMS` (TC2, step 5a): a callee that is a parameter is answered from the functions the key
    /// holds for it. `LOCALS` (TC3 step 2): a variable of the body holds the join of what is assigned to it. `CAPTURES` (TC3 step 3): a nested def's value
    /// carries the values of the variables it reads from around it. `MODULES` (TC3 step 4): a variable at the top of a file has its row of the file's own table as its
    /// value, read from a function of the file or through an import. `RELEVANCE` (TC3 step 6): a parameter that cannot reach a call, through variables
    /// and captures, is unknown in a key, so callers that differ only in it share a context. `OBJECTS` (TC4 step 4): an object's members are read through
    /// patterns (`const { a } = o`, a pattern parameter), members read into a name (`const f = o.a`), and captured objects; a member *call* is step 5's.
    pub(crate) const BUILT: Caps = Caps::PARAMS
        .union(Caps::LOCALS)
        .union(Caps::CAPTURES)
        .union(Caps::MODULES)
        .union(Caps::RELEVANCE)
        .union(Caps::OBJECTS);
}
