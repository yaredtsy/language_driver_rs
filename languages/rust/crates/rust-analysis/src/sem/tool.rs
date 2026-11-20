//! Where `cargo`, `rustc` and `rustup` are: rustup's proxy first, so a `rust-toolchain.toml` (or `RUSTUP_TOOLCHAIN`) is read and the engine and this crate
//! agree about which Rust there is (RS3 §6, ENGINE.md "Finding a tool"). An owned path comes out, no engine type.
//!
//! The order is the engine's `Tool::prefer_proxy` (`$CARGO_HOME/bin/<tool>`, the `CARGO` / `RUSTC` variable, `PATH`) with `PATH` moved **before** the
//! variable. Under `cargo run` or `cargo test` the variable is the toolchain's own binary, which is not rustup's proxy and reads no pin, and a rustup
//! whose proxies are not in `$CARGO_HOME/bin` (a package-manager build, such as Homebrew's, keeps them on `PATH`) would let it win: a crate pinned to a
//! toolchain that is missing then probes as the outer toolchain, is not substituted, and fails in `rustc -vV`.
//!
//! Not `Tool::path`: it takes the variable first, which loses the pin the same way.

use std::env;
use std::path::PathBuf;

use ra_ap_toolchain::Tool as Engine;

use crate::toolchain::Tool;

/// The program to start for `tool`: a path when one is found, else what the engine falls back to (the variable, then the bare name).
pub(crate) fn path(tool: Tool) -> PathBuf {
    let engine = match tool {
        Tool::Cargo => Engine::Cargo,
        Tool::Rustc => Engine::Rustc,
        Tool::Rustup => Engine::Rustup,
    };
    if let Some(proxy) = engine.proxy() {
        return proxy.into_std_path_buf();
    }
    in_path(engine.name()).unwrap_or_else(|| engine.prefer_proxy().into_std_path_buf())
}

/// The first `PATH` folder that holds `name` (with `.exe` where the platform has an extension for programs).
fn in_path(name: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    env::split_paths(&paths).find_map(|folder| {
        let plain = folder.join(name);
        let with_extension = match env::consts::EXE_EXTENSION {
            "" => None,
            extension => Some(plain.with_extension(extension)),
        };
        std::iter::once(plain)
            .chain(with_extension)
            .find(|candidate| candidate.is_file())
    })
}
