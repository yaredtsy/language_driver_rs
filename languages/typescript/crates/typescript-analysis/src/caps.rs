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
    }
}

impl Caps {
    /// The bits that are built: what `Precision::Precise` turns on. `PARAMS` (TC2, step 5a): a callee that is a parameter is answered from the functions the key
    /// holds for it. `LOCALS` (TC3 step 2): a variable of the body holds the join of what is assigned to it.
    pub(crate) const BUILT: Caps = Caps::PARAMS.union(Caps::LOCALS);
}
