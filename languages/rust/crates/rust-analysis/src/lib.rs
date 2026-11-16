//! SLOW: project loading, call sites, call graph. The `sem` folder is the only code that imports the checker.

mod paths;
mod project;
mod sem;
mod toolchain;

pub use project::{OpenError, Project};
