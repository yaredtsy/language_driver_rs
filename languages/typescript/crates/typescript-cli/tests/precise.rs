//! `typescript-cli calls … --precise`: the context layer on. TC1 step 1 builds the switch and nothing behind it, so every
//! output is byte for byte the output without the flag.

use std::path::PathBuf;
use std::process::{Command, Output};

fn scenario(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../scenarios")
        .join(name)
        .join("typescript")
}

/// `typescript-cli calls <scenario>/typescript main.ts --entry main <flags>`.
fn calls(scenario_name: &str, flags: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_typescript-cli"))
        .arg("calls")
        .arg(scenario(scenario_name))
        .args(["main.ts", "--entry", "main"])
        .args(flags)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

#[test]
fn precise_prints_what_no_flag_prints() {
    let scenarios = [
        "construct-marked-class",
        "external-call",
        "method-on-class",
        "recursion-direct",
        "straight-line",
        "two-callers",
    ];
    let outputs: [&[&str]; 4] = [&[], &["--wire"], &["--wire", "--merged"], &["--graph"]];
    for name in scenarios {
        for flags in outputs {
            let plain = calls(name, flags);
            let mut with = flags.to_vec();
            with.push("--precise");
            let precise = calls(name, &with);
            let at = format!("{name} {flags:?}");
            assert_eq!(stdout(&plain), stdout(&precise), "stdout: {at}");
            assert_eq!(stderr(&plain), stderr(&precise), "stderr: {at}");
        }
    }
}

#[test]
fn the_usage_names_precise() {
    let out = Command::new(env!("CARGO_BIN_EXE_typescript-cli"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("--precise"), "{}", stderr(&out));
}
