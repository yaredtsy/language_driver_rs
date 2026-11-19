//! SLOW: project loading, call sites, call graph. The `sem` folder is the only code that imports the checker.

mod calls;
mod env;
mod key;
mod manifest;
mod paths;
mod project;
mod sem;
mod session;
mod toolchain;
mod walk;
mod warning;

pub use analyzer_core::graph::{
    Basis, Context, ContextId, Cut, DefInfo, Edge, EdgeTo, Entry, External, Graph, Reason, Root,
    Site, StableDef, Stats, Status,
};
pub use analyzer_core::render::{Mark, Node, RenderOptions, render, to_text};
pub use analyzer_core::solve::{Budget, Cancel, Limits};
pub use analyzer_core::wire::{Frame, LineSpan, Sources, WireOptions, WireStyle, merge, wire};
pub use calls::{Breakdown, Options, breakdown, call_graph};
pub use project::{DefMatch, LoadOptions, OpenError, Project};
pub use rust_syntax::{DefKind, MarkerStyle, Placement, Span};
pub use session::{Refreshed, Session};
pub use warning::{Warning, Why};
