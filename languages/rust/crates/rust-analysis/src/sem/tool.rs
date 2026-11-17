//! Where `cargo`, `rustc` and `rustup` are: the engine's own rule (`ra_ap_toolchain::Tool::path`: the `CARGO` / `RUSTC` variable, then `PATH`, then
//! `$CARGO_HOME/bin`), so the engine and this crate never disagree about which Rust there is (RS3 §6, ENGINE.md "Finding a tool"). An owned path comes
//! out, no engine type.

use std::path::PathBuf;

use ra_ap_toolchain::Tool as Engine;

use crate::toolchain::Tool;

/// The program to start for `tool`: a path when the engine finds one, else the bare name, which is what the engine itself falls back to.
pub(crate) fn path(tool: Tool) -> PathBuf {
    match tool {
        Tool::Cargo => Engine::Cargo,
        Tool::Rustc => Engine::Rustc,
        Tool::Rustup => Engine::Rustup,
    }
    .path()
    .into_std_path_buf()
}
