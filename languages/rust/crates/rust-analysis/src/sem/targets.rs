//! When creating or deleting a `.rs` file changes the crate graph (RS5 §3): rust-analyzer's own rule, copied. Cargo finds a target by itself at some
//! places (`build.rs`, `src/main.rs`, `src/lib.rs`, a file in `src/bin`, `examples`, `tests`, `benches`, or a `main.rs` one folder below them), so a file
//! there that appears or goes is a new or a lost crate, and the workspace is loaded again. Anywhere else it is a module, and the fast path handles it.
//!
//! `should_refresh_for_change` is `crates/rust-analyzer/src/reload.rs` of rust-analyzer, which is not in a published `ra_ap_*` crate, as it stands:
//! the matching is a plain `ends_with` on the path's text, so `mysrc/lib.rs` and `mytests/a.rs` match too. Copied, not improved: the engine's own
//! answer is the one we follow ("follow the engine").

use ra_ap_vfs::{AbsPath, ChangeKind};

pub(super) fn should_refresh_for_change(
    path: &AbsPath,
    change_kind: ChangeKind,
    additional_paths: &[&str],
) -> bool {
    // Note: build scripts are retriggered on file save, no refresh is necessary
    const IMPLICIT_TARGET_FILES: &[&str] = &["build.rs", "src/main.rs", "src/lib.rs"];
    const IMPLICIT_TARGET_DIRS: &[&str] = &["src/bin", "examples", "tests", "benches"];

    let file_name = match path.file_name() {
        Some(it) => it,
        None => return false,
    };

    if let "Cargo.toml" | "Cargo.lock" = file_name {
        return true;
    }

    if additional_paths.contains(&file_name) {
        return true;
    }

    // .cargo/config{.toml}
    if matches!(file_name, "config.toml" | "config")
        && path
            .parent()
            .map(|parent| parent.as_str().ends_with(".cargo"))
            .unwrap_or(false)
    {
        return true;
    }

    // Everything below only matters when files are created or deleted
    if change_kind == ChangeKind::Modify {
        return false;
    }

    if path.extension().unwrap_or_default() != "rs" {
        return false;
    }

    if IMPLICIT_TARGET_FILES
        .iter()
        .any(|it| path.as_str().ends_with(it))
    {
        return true;
    }
    let parent = match path.parent() {
        Some(it) => it,
        None => return false,
    };
    if IMPLICIT_TARGET_DIRS
        .iter()
        .any(|it| parent.as_str().ends_with(it))
    {
        return true;
    }
    if file_name == "main.rs" {
        let grand_parent = match parent.parent() {
            Some(it) => it,
            None => return false,
        };
        if IMPLICIT_TARGET_DIRS
            .iter()
            .any(|it| grand_parent.as_str().ends_with(it))
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    //! Each branch of the rule, by the kind of change and the path.

    use ra_ap_vfs::AbsPathBuf;

    use super::*;

    const ALL: [ChangeKind; 3] = [ChangeKind::Create, ChangeKind::Modify, ChangeKind::Delete];

    /// `/w/src/lib.rs` is absolute where paths start at the root, and not on Windows, which wants a drive: `C:/w/src/lib.rs`. The cases below are written
    /// with the short form; the rule matches the text with `/`, as the engine's paths have it, so the forward slashes stay.
    fn platform_absolute(path: &str) -> String {
        if cfg!(windows) {
            format!("C:{path}")
        } else {
            path.to_owned()
        }
    }

    fn refresh(path: &str, kind: ChangeKind, additional: &[&str]) -> bool {
        let path =
            AbsPathBuf::try_from(platform_absolute(path).as_str()).expect("an absolute path");
        should_refresh_for_change(&path, kind, additional)
    }

    /// The kinds that make the path refresh: `[create, modify, delete]`.
    fn kinds(path: &str) -> [bool; 3] {
        ALL.map(|kind| refresh(path, kind, &[]))
    }

    const ALWAYS: [bool; 3] = [true, true, true];
    const CREATE_AND_DELETE: [bool; 3] = [true, false, true];
    const NEVER: [bool; 3] = [false, false, false];

    #[test]
    fn a_manifest_or_a_lock_refreshes_whatever_the_change() {
        assert_eq!(kinds("/w/Cargo.toml"), ALWAYS);
        assert_eq!(kinds("/w/member/Cargo.lock"), ALWAYS);
        assert_eq!(kinds("/w/cargo.toml"), NEVER, "the name is exact");
    }

    #[test]
    fn an_additional_name_refreshes_whatever_the_change() {
        for kind in ALL {
            assert!(refresh(
                "/w/rust-toolchain.toml",
                kind,
                &["rust-toolchain.toml"]
            ));
            assert!(!refresh("/w/rust-toolchain.toml", kind, &[]));
            assert!(!refresh("/w/other.toml", kind, &["rust-toolchain.toml"]));
        }
    }

    #[test]
    fn a_cargo_config_refreshes_whatever_the_change() {
        assert_eq!(kinds("/w/.cargo/config.toml"), ALWAYS);
        assert_eq!(kinds("/w/.cargo/config"), ALWAYS);
        assert_eq!(
            kinds("/w/cargo/config.toml"),
            NEVER,
            "not a `.cargo` folder"
        );
        assert_eq!(kinds("/w/.cargo/other.toml"), NEVER);
        // plain `ends_with`: any folder whose name ends in `.cargo`
        assert_eq!(kinds("/w/not.cargo/config.toml"), ALWAYS);
    }

    #[test]
    fn a_saved_rs_file_never_refreshes() {
        for path in [
            "/w/build.rs",
            "/w/src/main.rs",
            "/w/src/lib.rs",
            "/w/src/bin/tool.rs",
            "/w/tests/it.rs",
            "/w/src/module.rs",
        ] {
            assert!(!refresh(path, ChangeKind::Modify, &[]), "{path}");
        }
    }

    #[test]
    fn a_file_that_is_not_rs_refreshes_only_for_the_names_above() {
        assert_eq!(kinds("/w/src/lib.txt"), NEVER);
        assert_eq!(kinds("/w/src/bin/tool.toml"), NEVER);
        assert_eq!(kinds("/w/src/lib"), NEVER, "no extension");
    }

    #[test]
    fn the_implicit_target_files_refresh_when_created_or_deleted() {
        assert_eq!(kinds("/w/build.rs"), CREATE_AND_DELETE);
        assert_eq!(kinds("/w/src/main.rs"), CREATE_AND_DELETE);
        assert_eq!(kinds("/w/src/lib.rs"), CREATE_AND_DELETE);
        assert_eq!(kinds("/w/member/src/lib.rs"), CREATE_AND_DELETE);
        assert_eq!(kinds("/w/src/other.rs"), NEVER, "a module");
        assert_eq!(kinds("/w/src/nested/lib.rs"), NEVER, "not `src/lib.rs`");
    }

    #[test]
    fn a_file_in_an_implicit_target_folder_refreshes_when_created_or_deleted() {
        assert_eq!(kinds("/w/src/bin/tool.rs"), CREATE_AND_DELETE);
        assert_eq!(kinds("/w/examples/demo.rs"), CREATE_AND_DELETE);
        assert_eq!(kinds("/w/tests/it.rs"), CREATE_AND_DELETE);
        assert_eq!(kinds("/w/benches/speed.rs"), CREATE_AND_DELETE);
        assert_eq!(
            kinds("/w/src/bin/tool/helper.rs"),
            NEVER,
            "two folders down"
        );
        assert_eq!(kinds("/w/src/binary/tool.rs"), NEVER);
    }

    #[test]
    fn a_main_rs_one_folder_below_a_target_folder_refreshes() {
        assert_eq!(kinds("/w/src/bin/tool/main.rs"), CREATE_AND_DELETE);
        assert_eq!(kinds("/w/examples/demo/main.rs"), CREATE_AND_DELETE);
        assert_eq!(kinds("/w/tests/suite/main.rs"), CREATE_AND_DELETE);
        assert_eq!(kinds("/w/benches/speed/main.rs"), CREATE_AND_DELETE);
        assert_eq!(kinds("/w/tests/suite/other.rs"), NEVER, "only `main.rs`");
        assert_eq!(kinds("/w/other/suite/main.rs"), NEVER);
        assert_eq!(kinds("/w/main.rs"), NEVER, "not under a target folder");
    }

    /// The matching is `ends_with` on the text, so a name that only ends like a target's matches. This is the engine's behaviour, copied on purpose.
    #[test]
    fn the_matching_is_a_plain_ends_with() {
        assert_eq!(kinds("/w/mysrc/lib.rs"), CREATE_AND_DELETE);
        assert_eq!(kinds("/w/my_build.rs"), CREATE_AND_DELETE);
        assert_eq!(kinds("/w/mytests/it.rs"), CREATE_AND_DELETE);
        assert_eq!(kinds("/w/myexamples/demo/main.rs"), CREATE_AND_DELETE);
    }

    /// A path with no file name or no parent: the rule says no.
    #[test]
    fn a_path_without_a_name_or_a_parent_does_not_refresh() {
        assert_eq!(kinds("/"), NEVER);
        assert_eq!(kinds("/lib.rs"), NEVER, "no parent folder to match");
    }
}
