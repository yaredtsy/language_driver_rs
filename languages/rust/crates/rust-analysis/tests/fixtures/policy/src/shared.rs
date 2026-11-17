macro_rules! from_elsewhere {
    ($e:expr) => {{
        $crate::shared::elsewhere();
        $e
    }};
}

// A project macro defined away from the module that uses it. The call in its body is written in this file, at an offset that is also inside the def
// that uses the macro (calls.rs, near its top): a range counts as a place in a body only when its file is the def's.
pub fn elsewhere() {}
