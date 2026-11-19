//! A session: a project kept open for many requests, each with its own budget (`docs/03-typescript/07-ts5-driver-needs.md` §2), and kept up to date with the
//! disk (`docs/03-typescript/11-step6-text-and-disk.md`).
//!
//! The `Project`, the `Options`, and what it takes to know the project is out of date: the stamps of the paths its last build depended on. The stamps
//! live here and not in the `Project`, so a session whose rebuild failed (no program) still knows what to watch and comes back by itself.
//!
//! A request builds its `Solver` fresh, because a `Solver` borrows the project and a struct cannot borrow from itself.

use std::path::{Path, PathBuf};

use analyzer_core::graph::{Entry, Graph};
use analyzer_core::solve::{Budget, Cancel};

use crate::calls::{Options, graph_within};
use crate::sem::Stamps;
use crate::{OpenError, Project};

/// A loaded project and how to read it. `Send` and not `Sync`, like the `Project` it holds: it moves to a worker thread, and two threads cannot ask
/// it at once.
pub struct Session {
    /// As `open` was given them: a rebuild opens from the same two, never from a file found again (the config is found from the file).
    root: PathBuf,
    file: PathBuf,
    options: Options,
    /// `None` while the last rebuild failed: `lost` says why.
    project: Option<Project>,
    lost: Option<OpenError>,
    /// The paths the last build depended on, as the disk was when the session last looked (see `Stamps::after_build`).
    stamps: Stamps,
}

/// What a [`Session::refresh`] or [`Session::reload`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Refreshed {
    /// The program was built again by this call: the next request is as slow as the first.
    pub reopened: bool,
    /// The session has no program, and why (the file it was opened from is gone, its config does not parse, no config holds the file any more). Every
    /// request answers `None`. Reported by **every** `refresh` and `reload` while it lasts, not only the first.
    pub lost: Option<OpenError>,
}

impl Session {
    /// Loads the project that owns `file` (see [`Project::open`]). `options` are the limits and the marker style of every request. A first load that fails
    /// gives no session: open again when asked again. Only a session that was open once can lose its program and get it back.
    pub fn open(root: &Path, file: &Path, options: Options) -> Result<Session, OpenError> {
        let (built, paths) = Project::open_tracked(root, file);
        Ok(Session {
            root: root.to_path_buf(),
            file: file.to_path_buf(),
            options,
            project: Some(built?),
            lost: None,
            stamps: Stamps::of_all(&paths),
        })
    }

    /// Brings the session up to date with the disk; call it before each request. Builds the program again when a path the last build depended on changed
    /// (a source file saved, created or deleted, a config, a lock file). Otherwise it only compares (milliseconds). Never fails: a rebuild that cannot be done
    /// leaves the session with no program, and `lost` says why on every call until one works.
    ///
    /// Only saved text is read (D42): a file the editor holds changed and not saved is not seen. Nor is a change inside `node_modules` that touched no lock
    /// file and added no folder: [`Session::reload`] sees that one.
    pub fn refresh(&mut self) -> Refreshed {
        let before = self.stamps.restamped();
        if before == self.stamps {
            return Refreshed {
                reopened: false,
                lost: self.lost.clone(),
            };
        }
        self.rebuild(before)
    }

    /// Builds the program again whatever the stamps say: for a change [`Session::refresh`] does not see.
    pub fn reload(&mut self) -> Refreshed {
        let before = self.stamps.restamped();
        self.rebuild(before)
    }

    /// `before` is what the disk said of the session's paths just now, before anything is built.
    fn rebuild(&mut self, before: Stamps) -> Refreshed {
        // The old program is freed first, so two are never alive at once. (`None` drops the `Project`, and `Sem`'s `Drop` gives the program back.)
        self.project = None;
        let (built, paths) = Project::open_tracked(&self.root, &self.file);
        // A path the session knew keeps the stamp from before the build: a file saved while the build ran is then seen by the next look. A new path is
        // stamped now.
        self.stamps = before.after_build(&paths);
        match built {
            Ok(project) => {
                self.project = Some(project);
                self.lost = None;
                Refreshed {
                    reopened: true,
                    lost: None,
                }
            }
            Err(error) => {
                self.lost = Some(error.clone());
                Refreshed {
                    reopened: false,
                    lost: Some(error),
                }
            }
        }
    }

    /// One request: the graph under `entry`, a def in `file` (relative to the root), walked no further than `budget`. A context past the budget is
    /// listed and not walked (`Status::Frontier`), and its node is drawn `Mark::Cut`: asking for that def as a new entry continues from there.
    /// `None` when there is no such def, and while the session has no program. With `Budget::default()` it is [`call_graph`](crate::call_graph)'s graph.
    pub fn graph(
        &self,
        file: &str,
        entry: &Entry<'_>,
        budget: &Budget,
        cancel: &Cancel,
    ) -> Option<Graph> {
        graph_within(
            self.project.as_ref()?,
            file,
            entry,
            &self.options,
            budget,
            cancel,
        )
    }

    /// The loaded project, or `None` while the session has no program (see [`Refreshed::lost`]).
    pub fn project(&self) -> Option<&Project> {
        self.project.as_ref()
    }
}

#[cfg(test)]
mod tests {
    //! The two tests that need a hook inside `sem`: a change that lands during a build, and a count of builds.

    use analyzer_core::solve::{Budget, Cancel};
    use typescript_syntax::MarkerStyle;

    use super::*;
    use crate::scratch::{scratch, write};
    use crate::sem::{BUILDS, after_the_next_bind};

    const MAIN: &str =
        "import { callee } from \"./lib\";\nexport function main() {\n  callee();\n}\n";
    const LIB: &str = "export function callee() {\n  first();\n}\nexport function first() {}\nexport function second() {}\n";
    /// `LIB` with `callee` calling `second`: one character longer.
    const LIB_SECOND: &str = "export function callee() {\n  second();\n}\nexport function first() {}\nexport function second() {}\n";

    fn project(name: &str) -> PathBuf {
        let root = scratch(name);
        std::fs::create_dir_all(root.join("src")).unwrap();
        write(
            root.join("tsconfig.json"),
            r#"{ "compilerOptions": { "strict": true }, "include": ["src"] }"#,
        );
        write(root.join("src/main.ts"), MAIN);
        write(root.join("src/lib.ts"), LIB);
        root
    }

    fn open(root: &Path) -> Session {
        Session::open(
            root,
            Path::new("src/main.ts"),
            Options::new(MarkerStyle::docstring("ID")),
        )
        .unwrap()
    }

    /// What `main` calls, as `callee`'s own callee: `first` or `second`, read from the graph.
    fn callee_calls(session: &Session) -> String {
        let entry = Entry::Def {
            qualname: "main",
            occurrence: 0,
        };
        let graph = session
            .graph("src/main.ts", &entry, &Budget::default(), &Cancel::new())
            .expect("main is there");
        let mut names: Vec<_> = graph
            .defs
            .iter()
            .map(|def| def.qname.clone())
            .filter(|name| name == "first" || name == "second")
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        names.pop().unwrap()
    }

    /// Case 13. The build reads `lib.ts` as it is, and `lib.ts` is saved again before the build ends. That `refresh` may give either tree (the program holds
    /// the text it read); the next one must find the file changed and build again. The stamp of a path the session knew is the one taken before the
    /// build: a stamp taken after it would describe the new text, and the old program would stay for good.
    #[test]
    fn a_file_rewritten_during_a_rebuild_is_seen_by_the_next_refresh() {
        let root = project("during-a-build");
        let mut session = open(&root);
        assert_eq!(callee_calls(&session), "first");

        // something that makes the next `refresh` build (the length of `main.ts` changes)
        write(root.join("src/main.ts"), &format!("{MAIN}\n"));
        let lib = root.join("src/lib.ts");
        after_the_next_bind(move || write(lib, LIB_SECOND));
        let first = session.refresh();
        assert!(first.reopened);
        assert!(["first", "second"].contains(&callee_calls(&session).as_str()));

        let second = session.refresh();
        assert!(second.reopened, "the file was saved during the build");
        assert_eq!(second.lost, None);
        assert_eq!(callee_calls(&session), "second");

        let third = session.refresh();
        assert!(!third.reopened, "and now nothing changed");
    }

    /// A lost session rebuilds when a stamp changed and not before each request (design §5): nothing is built by a `refresh` of a session that is lost
    /// for good, and `reload` builds.
    #[test]
    fn a_lost_session_does_not_rebuild_when_nothing_changed() {
        let root = project("lost-no-rebuild");
        let mut session = open(&root);
        std::fs::remove_file(root.join("src/main.ts")).unwrap();

        let builds = || BUILDS.with(std::cell::Cell::get);
        let before = builds();
        assert_eq!(session.refresh().lost, Some(OpenError::NoSuchFile));
        assert_eq!(builds(), before + 1, "the deletion is a change: one build");
        for _ in 0..3 {
            assert_eq!(session.refresh().lost, Some(OpenError::NoSuchFile));
        }
        assert_eq!(builds(), before + 1, "nothing changed: nothing built");

        assert_eq!(session.reload().lost, Some(OpenError::NoSuchFile));
        assert_eq!(
            builds(),
            before + 2,
            "reload builds whatever the stamps say"
        );

        write(root.join("src/main.ts"), MAIN);
        let back = session.refresh();
        assert!(back.reopened);
        assert_eq!(builds(), before + 3);
    }

    /// `Stamp::of` tells a file, a folder and a path that is not there apart, and a file's stamp moves with its length.
    #[test]
    fn a_stamp_tells_file_folder_and_missing_apart() {
        use crate::sem::Stamps;

        let root = scratch("stamps");
        let file = root.join("a.txt");
        write(&file, "one");
        let paths: Vec<String> = [&root, &file, &root.join("none.txt")]
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        let stamps = Stamps::of_all(&paths);
        assert_eq!(stamps, stamps.restamped());

        write(&file, "three");
        assert_ne!(stamps, stamps.restamped(), "the length moved");
        // a path known keeps its stamp, one that is new is stamped now
        let mut more = paths.clone();
        more.push(root.join("b.txt").to_string_lossy().into_owned());
        let after = stamps.after_build(&more);
        assert_eq!(after.paths().count(), 4);
        assert_ne!(after, after.restamped(), "a.txt keeps the stamp of before");
    }
}
