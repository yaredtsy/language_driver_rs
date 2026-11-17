//! Where `cargo`, `rustc` and `rustup` are: the engine's rule for a load that has a sysroot (`Sysroot::tool` uses `ra_ap_toolchain::Tool::prefer_proxy`:
//! `$CARGO_HOME/bin/<tool>` first, the rustup proxy, then the `CARGO` / `RUSTC` variable, then `PATH`), so the engine and this crate never disagree about
//! which Rust there is (RS3 §6, ENGINE.md "Finding a tool"). An owned path comes out, no engine type.
//!
//! Not `Tool::path`: it takes the `CARGO` variable first, and under `cargo run` or `cargo test` that is the toolchain's own binary, which is not rustup's
//! proxy, so a `rust-toolchain.toml` (or `RUSTUP_TOOLCHAIN`) is not read and a crate pinned to a toolchain that is missing loads with no `std`.

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
    .prefer_proxy()
    .into_std_path_buf()
}
