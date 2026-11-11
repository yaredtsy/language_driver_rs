//! SLOW: project loading, call sites, call graph. `sem.rs` is the only file that imports the type checker.
//!
//! Moved from pylspt (`670ae79a15`). Modules are public for now, as they were there; PY5's later steps make them private one at a
//! time, and `unreachable_pub` says which `pub` was an accident.

pub mod classes;
pub mod interp;
pub mod project;
pub mod sem;
pub mod stable;
pub mod wire;
