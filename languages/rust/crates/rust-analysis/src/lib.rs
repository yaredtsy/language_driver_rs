//! SLOW: project loading, call sites, call graph. The `sem` folder is the only code that imports the checker.

mod env;
mod key;
mod manifest;
mod paths;
mod project;
mod sem;
mod toolchain;
mod walk;
mod warning;

pub use project::{LoadOptions, OpenError, Project};
pub use warning::{Warning, Why};
