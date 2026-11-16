//! SLOW: project loading, call sites, call graph. The `sem` folder is the only code that imports the checker.

mod project;
mod sem;

pub use project::{OpenError, Project};
