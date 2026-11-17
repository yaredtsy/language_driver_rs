//! SLOW: project loading, call sites, call graph. The `sem` folder is the only code that imports the checker.

mod paths;
mod project;
mod sem;
mod toolchain;
mod warning;

pub use project::{LoadOptions, OpenError, Project};
pub use warning::{Warning, Why};
