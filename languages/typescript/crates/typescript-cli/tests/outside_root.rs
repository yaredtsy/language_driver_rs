//! D33 through the binary: a file outside the root is an error with a message, not a panic and not a tree of absolute paths.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn calls_on_a_file_outside_the_root_fails_with_the_error_text() {
    let app = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../typescript-analysis/tests/fixtures/outside-root/app");
    let out = Command::new(env!("CARGO_BIN_EXE_typescript-cli"))
        .args([
            "calls",
            app.to_str().unwrap(),
            "../shared/lib.ts",
            "--entry",
            "helper",
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("the file is outside the root"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(out.stdout.is_empty());
}
