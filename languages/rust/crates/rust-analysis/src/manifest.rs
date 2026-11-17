//! Our own reader of `Cargo.toml` (RS3 §6, the ladder's rung 3): the crate graph of a Cargo workspace, written from Cargo's documented rules, for
//! when cargo cannot be asked. No engine type here: `Layout` is plain data that `sem` turns into the engine's project description.
//!
//! The reader never guesses. A package, a dependency or a target it does not understand is **skipped and counted** (`Layout::skipped`, reported as
//! `layout_skipped`), never invented (rule 8).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::paths;

/// The filesystem as far as the reader needs it, so that its rules can be tested on a tree in memory.
pub(crate) trait Fs {
    fn read(&self, path: &Path) -> Option<String>;
    fn is_file(&self, path: &Path) -> bool;
    /// The names of what is in `dir`, sorted. Empty when there is no such folder.
    fn list(&self, dir: &Path) -> Vec<String>;
}

pub(crate) struct DiskFs;

impl Fs for DiskFs {
    fn read(&self, path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn list(&self, dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .collect();
        names.sort();
        names
    }
}

/// One crate the engine is told about: a target of a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CrateSpec {
    /// The crate's name in code: a lib's `[lib] name` or the package's, a bin's or test's own; `-` is `_`.
    pub(crate) name: String,
    /// Absolute.
    pub(crate) root_module: PathBuf,
    /// `2015`, `2018`, `2021` or `2024`.
    pub(crate) edition: &'static str,
    /// (the name the crate is called by, the index of the crate in `Layout::crates`)
    pub(crate) deps: Vec<(String, usize)>,
    /// The folder of the package: all of it is the crate's source (a `#[path]` module may sit outside `src/`), minus its `target/`.
    pub(crate) package_dir: PathBuf,
    /// `unix`, `target_os="linux"`, `debug_assertions`, `feature="x"`, …: what `rustc` would have said, and the package's default features.
    pub(crate) cfg: Vec<String>,
    pub(crate) is_proc_macro: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Layout {
    pub(crate) crates: Vec<CrateSpec>,
    /// What the reader did not understand or could not load, counted: `layout_skipped`.
    pub(crate) skipped: u64,
    /// Every `Cargo.toml` that was read (absolute, sorted): what a change to means "load again".
    pub(crate) manifests: Vec<PathBuf>,
    /// The folder of the workspace's root manifest, where `Cargo.lock` is.
    pub(crate) workspace_dir: PathBuf,
}

/// What `rustc --print cfg` would say for the machine this crate was built for, as far as `std` tells, plus `debug_assertions`. With no `rustc` the
/// engine has none, and `#[cfg(unix)]` code would be inactive. An approximation: the project is assumed to build for this machine.
pub(crate) fn host_cfgs() -> Vec<String> {
    let mut cfg = Vec::new();
    if cfg!(unix) {
        cfg.push("unix".to_owned());
    }
    if cfg!(windows) {
        cfg.push("windows".to_owned());
    }
    cfg.push(format!("target_os=\"{}\"", std::env::consts::OS));
    cfg.push(format!("target_arch=\"{}\"", std::env::consts::ARCH));
    cfg.push(format!("target_family=\"{}\"", std::env::consts::FAMILY));
    cfg.push(format!("target_pointer_width=\"{}\"", usize::BITS));
    cfg.push(format!(
        "target_endian=\"{}\"",
        if cfg!(target_endian = "little") {
            "little"
        } else {
            "big"
        }
    ));
    cfg.push("debug_assertions".to_owned());
    cfg
}

fn crate_name(name: &str) -> String {
    name.replace('-', "_")
}

/// The package's name, which is also its folder's identity in `Reader::packages`.
struct Package {
    dir: PathBuf,
    name: String,
    toml: Table,
    edition: &'static str,
    /// A member of the workspace (it gets its dev-dependencies and its `tests/`), as opposed to a path dependency found along the way.
    member: bool,
    /// The features that are on by default, and the optional dependencies they turn on.
    features: BTreeSet<String>,
    optional_on: BTreeSet<String>,
    lib: Option<usize>,
    targets: Vec<usize>,
}

struct Reader<'a> {
    fs: &'a dyn Fs,
    workspace_dir: PathBuf,
    workspace: Table,
    packages: Vec<Package>,
    by_dir: BTreeMap<PathBuf, usize>,
    crates: Vec<CrateSpec>,
    skipped: u64,
    manifests: BTreeSet<PathBuf>,
    host: Vec<String>,
}

/// The crate graph of the workspace that `manifest` belongs to. `Err` carries why the manifest cannot be read at all (not TOML, or no package and no
/// workspace in it, or nothing in it that the reader can load): rung 4 then takes over.
pub(crate) fn read(fs: &dyn Fs, manifest: &Path) -> Result<Layout, String> {
    let text = fs
        .read(manifest)
        .ok_or_else(|| format!("{}: cannot be read", manifest.display()))?;
    let table: Table = toml::from_str(&text)
        .map_err(|e| format!("{}: {}", manifest.display(), one_line(&e.to_string())))?;
    let dir = manifest
        .parent()
        .ok_or_else(|| format!("{}: has no folder", manifest.display()))?
        .to_path_buf();

    let (workspace_dir, workspace) = find_workspace(fs, &dir, &table);
    let mut reader = Reader {
        fs,
        workspace_dir: workspace_dir.clone(),
        workspace: workspace.clone(),
        packages: Vec::new(),
        by_dir: BTreeMap::new(),
        crates: Vec::new(),
        skipped: 0,
        manifests: BTreeSet::new(),
        host: host_cfgs(),
    };
    reader.manifests.insert(manifest.to_path_buf());
    if workspace_dir.join("Cargo.toml") != manifest {
        reader.manifests.insert(workspace_dir.join("Cargo.toml"));
    }
    // `[patch]` and `[replace]` sit at the top of the root manifest, not in `[workspace]`: they change which source a dependency has, which the
    // reader does not follow
    let root_manifest = if workspace_dir == dir {
        Some(table.clone())
    } else {
        fs.read(&workspace_dir.join("Cargo.toml"))
            .and_then(|t| toml::from_str(&t).ok())
    };
    if let Some(root) = root_manifest {
        reader.skipped += ["patch", "replace"]
            .iter()
            .filter(|key| root.contains_key(**key))
            .count() as u64;
    }
    for member in reader.member_dirs(&dir, &table) {
        reader.discover(&member, true);
    }
    reader.build_crates();
    reader.resolve_dependencies();
    if reader.crates.is_empty() {
        return Err(format!(
            "{}: nothing in it can be loaded (no package with a lib, a bin or a test the reader understands)",
            manifest.display()
        ));
    }
    Ok(Layout {
        crates: reader.crates,
        skipped: reader.skipped,
        manifests: reader.manifests.into_iter().collect(),
        workspace_dir,
    })
}

fn one_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_owned()
}

fn table(value: Option<&Value>) -> Option<&Table> {
    value.and_then(Value::as_table)
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}

/// The workspace a manifest belongs to: its own folder when it has `[workspace]`; else the nearest folder above with a `Cargo.toml` that has one and
/// lists this folder as a member (Cargo's rule); else the package alone.
fn find_workspace(fs: &dyn Fs, dir: &Path, own: &Table) -> (PathBuf, Table) {
    if let Some(workspace) = table(own.get("workspace")) {
        return (dir.to_path_buf(), workspace.clone());
    }
    for above in dir.ancestors().skip(1) {
        let Some(text) = fs.read(&above.join("Cargo.toml")) else {
            continue;
        };
        let Ok(parent) = toml::from_str::<Table>(&text) else {
            continue;
        };
        let Some(workspace) = table(parent.get("workspace")) else {
            continue;
        };
        if workspace_members(fs, above, workspace)
            .iter()
            .any(|member| member == dir)
        {
            return (above.to_path_buf(), workspace.clone());
        }
        // the nearest workspace above is the one that counts, and it does not list this package: the package is on its own
        break;
    }
    (dir.to_path_buf(), Table::new())
}

/// The folders `[workspace] members` names, minus `exclude`: a segment `*` is any one folder. Sorted. A pattern with another wildcard is left out
/// (the caller counts it).
fn workspace_members(fs: &dyn Fs, workspace_dir: &Path, workspace: &Table) -> Vec<PathBuf> {
    let mut found = BTreeSet::new();
    for pattern in strings(workspace.get("members")) {
        let mut candidates = vec![workspace_dir.to_path_buf()];
        for segment in pattern.split('/').filter(|s| !s.is_empty() && *s != ".") {
            let mut next = Vec::new();
            for base in &candidates {
                if segment == "*" {
                    next.extend(fs.list(base).into_iter().map(|name| base.join(name)));
                } else if !segment.contains(['*', '?', '[']) {
                    next.push(base.join(segment));
                }
            }
            candidates = next;
        }
        found.extend(
            candidates
                .into_iter()
                .filter(|dir| fs.is_file(&dir.join("Cargo.toml")))
                .map(|dir| paths::absolute_in(&dir, Path::new(""))),
        );
    }
    let excluded: BTreeSet<PathBuf> = strings(workspace.get("exclude"))
        .iter()
        .map(|e| paths::absolute_in(workspace_dir, Path::new(e)))
        .collect();
    found
        .into_iter()
        .filter(|dir| !excluded.contains(dir))
        .collect()
}

impl Reader<'_> {
    /// The packages to load: the root package (if the workspace manifest has `[package]`) and the members; for a manifest that is no workspace at all,
    /// its own package.
    fn member_dirs(&mut self, dir: &Path, own: &Table) -> Vec<PathBuf> {
        let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
        if self.workspace.is_empty() && !own.contains_key("workspace") {
            dirs.insert(dir.to_path_buf());
        } else {
            let root_toml = self
                .fs
                .read(&self.workspace_dir.join("Cargo.toml"))
                .and_then(|text| toml::from_str::<Table>(&text).ok());
            if root_toml
                .as_ref()
                .is_some_and(|t| t.contains_key("package"))
            {
                dirs.insert(self.workspace_dir.clone());
            }
            for pattern in strings(self.workspace.get("members")) {
                if pattern
                    .split('/')
                    .any(|s| s.contains("**") || (s != "*" && s.contains(['*', '?', '['])))
                {
                    self.skipped += 1;
                }
            }
            dirs.extend(workspace_members(
                self.fs,
                &self.workspace_dir,
                &self.workspace,
            ));
            // the manifest the caller gave belongs to this workspace, but a member list that does not name it still loads it (it is what was asked for)
            if table(own.get("package")).is_some() && dirs.is_empty() {
                dirs.insert(dir.to_path_buf());
            }
        }
        dirs.into_iter().collect()
    }

    /// Read the package in `dir` (once), and the path dependencies it has, which are packages too.
    fn discover(&mut self, dir: &Path, member: bool) {
        if let Some(&at) = self.by_dir.get(dir) {
            self.packages[at].member |= member;
            return;
        }
        let manifest = dir.join("Cargo.toml");
        let Some(toml) = self
            .fs
            .read(&manifest)
            .and_then(|text| toml::from_str::<Table>(&text).ok())
        else {
            // a package that cannot be read is not loaded, and is not invented
            self.skipped += 1;
            return;
        };
        self.manifests.insert(manifest);
        let Some(package) = table(toml.get("package")) else {
            // a manifest with only `[workspace]` is no package
            return;
        };
        let Some(name) = package.get("name").and_then(Value::as_str) else {
            self.skipped += 1;
            return;
        };
        let Some(edition) = self.edition_of(package) else {
            self.skipped += 1;
            return;
        };
        let (features, optional_on) = default_features(&toml);
        let name = name.to_owned();
        self.by_dir.insert(dir.to_path_buf(), self.packages.len());
        self.packages.push(Package {
            dir: dir.to_path_buf(),
            name,
            toml: toml.clone(),
            edition,
            member,
            features,
            optional_on,
            lib: None,
            targets: Vec::new(),
        });
        // path dependencies are packages of the project too: normal ones always, dev ones for a member
        let mut sections = vec!["dependencies"];
        if member {
            sections.push("dev-dependencies");
        }
        for section in sections {
            for (key, spec) in table(toml.get(section)).into_iter().flatten() {
                if let Some(dependency) = self.dependency_path(dir, key, spec) {
                    self.discover(&dependency, false);
                }
            }
        }
    }

    /// `edition = "2021"`, or the workspace's with `edition.workspace = true`; 2015 when absent, as Cargo. `None` for one the reader does not know.
    fn edition_of(&self, package: &Table) -> Option<&'static str> {
        let text = match package.get("edition") {
            None => "2015".to_owned(),
            Some(Value::String(edition)) => edition.clone(),
            Some(Value::Table(t)) if t.get("workspace").and_then(Value::as_bool) == Some(true) => {
                table(self.workspace.get("package"))
                    .and_then(|p| p.get("edition"))
                    .and_then(Value::as_str)
                    .unwrap_or("2015")
                    .to_owned()
            }
            Some(_) => return None,
        };
        ["2015", "2018", "2021", "2024"]
            .into_iter()
            .find(|known| *known == text)
    }

    /// The absolute folder a dependency spec points to with `path`, from the package's own table or, with `workspace = true`, from
    /// `[workspace.dependencies]` (whose paths are relative to the workspace's folder). `None` for a registry or git dependency: no source to load.
    fn dependency_path(&self, dir: &Path, key: &str, spec: &Value) -> Option<PathBuf> {
        let spec_table = spec.as_table()?;
        if spec_table.get("workspace").and_then(Value::as_bool) == Some(true) {
            let inherited = table(self.workspace.get("dependencies"))
                .and_then(|deps| deps.get(key))?
                .as_table()?;
            return Some(paths::absolute_in(
                &self.workspace_dir,
                Path::new(inherited.get("path")?.as_str()?),
            ));
        }
        Some(paths::absolute_in(
            dir,
            Path::new(spec_table.get("path")?.as_str()?),
        ))
    }

    /// Does the dependency spec say `package = "…"` (in the package's table, or in `[workspace.dependencies]` for `workspace = true`)?
    fn is_renamed(&self, key: &str, spec: &Value) -> bool {
        let Some(spec_table) = spec.as_table() else {
            return false;
        };
        if spec_table.contains_key("package") {
            return true;
        }
        spec_table.get("workspace").and_then(Value::as_bool) == Some(true)
            && table(self.workspace.get("dependencies"))
                .and_then(|deps| deps.get(key))
                .and_then(Value::as_table)
                .is_some_and(|t| t.contains_key("package"))
    }

    /// One crate per target of every package, in package order (a lib first). Dependencies come after, when every crate has its index.
    fn build_crates(&mut self) {
        for at in 0..self.packages.len() {
            let (dir, name, edition, member) = {
                let p = &self.packages[at];
                (p.dir.clone(), p.name.clone(), p.edition, p.member)
            };
            let package_table = table(self.packages[at].toml.get("package"))
                .cloned()
                .unwrap_or_default();
            let toml = self.packages[at].toml.clone();
            let mut cfg = self.host.clone();
            cfg.extend(
                self.packages[at]
                    .features
                    .iter()
                    .map(|f| format!("feature=\"{f}\"")),
            );

            let push = |this: &mut Self,
                        name: String,
                        root: PathBuf,
                        is_proc_macro: bool,
                        is_lib: bool| {
                let index = this.crates.len();
                this.crates.push(CrateSpec {
                    name,
                    root_module: root,
                    edition,
                    deps: Vec::new(),
                    package_dir: dir.clone(),
                    cfg: cfg.clone(),
                    is_proc_macro,
                });
                if is_lib {
                    this.packages[at].lib = Some(index);
                } else {
                    this.packages[at].targets.push(index);
                }
            };

            // the lib
            let lib = table(toml.get("lib"));
            let lib_path = lib
                .and_then(|l| l.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("src/lib.rs");
            if self.fs.is_file(&dir.join(lib_path)) {
                let lib_name = lib
                    .and_then(|l| l.get("name"))
                    .and_then(Value::as_str)
                    .map_or_else(|| crate_name(&name), crate_name);
                let proc_macro = lib
                    .and_then(|l| l.get("proc-macro"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                push(
                    self,
                    lib_name,
                    paths::absolute_in(&dir, Path::new(lib_path)),
                    proc_macro,
                    true,
                );
            } else if lib.is_some() {
                self.skipped += 1;
            }

            // bins and tests: found by Cargo's layout unless the package turns that off; each `[[bin]]` / `[[test]]` as well
            let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
            let auto = |key: &str| {
                package_table
                    .get(key)
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
            };
            let mut wanted: Vec<(String, PathBuf)> = Vec::new();
            if auto("autobins") {
                if self.fs.is_file(&dir.join("src/main.rs")) {
                    wanted.push((name.clone(), PathBuf::from("src/main.rs")));
                }
                wanted.extend(self.discovered(&dir, "src/bin"));
            }
            for entry in toml
                .get("bin")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_table)
            {
                let Some(bin_name) = entry.get("name").and_then(Value::as_str) else {
                    self.skipped += 1;
                    continue;
                };
                let default = if bin_name == name {
                    "src/main.rs".to_owned()
                } else {
                    format!("src/bin/{bin_name}.rs")
                };
                wanted.push((
                    bin_name.to_owned(),
                    PathBuf::from(
                        entry
                            .get("path")
                            .and_then(Value::as_str)
                            .unwrap_or(&default),
                    ),
                ));
            }
            if member && auto("autotests") {
                wanted.extend(self.discovered(&dir, "tests"));
            }
            if member {
                for entry in toml
                    .get("test")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_table)
                {
                    let Some(test_name) = entry.get("name").and_then(Value::as_str) else {
                        self.skipped += 1;
                        continue;
                    };
                    wanted.push((
                        test_name.to_owned(),
                        PathBuf::from(
                            entry
                                .get("path")
                                .and_then(Value::as_str)
                                .unwrap_or(&format!("tests/{test_name}.rs")),
                        ),
                    ));
                }
            }
            for (target_name, path) in wanted {
                if !seen.insert(path.clone()) {
                    continue;
                }
                if self.fs.is_file(&dir.join(&path)) {
                    push(
                        self,
                        crate_name(&target_name),
                        paths::absolute_in(&dir, &path),
                        false,
                        false,
                    );
                } else {
                    self.skipped += 1;
                }
            }

            // what the reader does not load: examples and benches, and a build script's output
            for folder in ["examples", "benches"] {
                self.skipped += self.discovered(&dir, folder).len() as u64;
            }
            if self.fs.is_file(&dir.join("build.rs"))
                || package_table
                    .get("build")
                    .is_some_and(|b| b.as_str().is_some())
            {
                self.skipped += 1;
            }
            // `[target.'cfg(unix)'.dependencies]`: which of them apply is a question for `rustc`
            self.skipped += table(toml.get("target")).map_or(0, |targets| targets.len() as u64);
        }
    }

    /// `<folder>/*.rs` and `<folder>/*/main.rs`, named by the file or the folder, sorted.
    fn discovered(&self, dir: &Path, folder: &str) -> Vec<(String, PathBuf)> {
        let base = dir.join(folder);
        let mut out = Vec::new();
        for entry in self.fs.list(&base) {
            if let Some(stem) = entry.strip_suffix(".rs") {
                out.push((stem.to_owned(), PathBuf::from(folder).join(&entry)));
            } else if self.fs.is_file(&base.join(&entry).join("main.rs")) {
                out.push((
                    entry.clone(),
                    PathBuf::from(folder).join(&entry).join("main.rs"),
                ));
            }
        }
        out
    }

    /// Fill in `CrateSpec::deps` now that every crate has an index: a lib gets its path dependencies' libs (and, for a member, its dev-dependencies':
    /// its own `#[cfg(test)]` code uses them); a bin or a test gets the same and its package's own lib.
    fn resolve_dependencies(&mut self) {
        for at in 0..self.packages.len() {
            let package_dir = self.packages[at].dir.clone();
            let member = self.packages[at].member;
            let toml = self.packages[at].toml.clone();
            let optional_on = self.packages[at].optional_on.clone();
            let mut deps: Vec<(String, usize)> = Vec::new();
            let mut sections = vec!["dependencies"];
            if member {
                sections.push("dev-dependencies");
            }
            for section in sections {
                for (key, spec) in table(toml.get(section)).into_iter().flatten() {
                    let optional = spec
                        .as_table()
                        .and_then(|t| t.get("optional"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if optional && !optional_on.contains(key) {
                        self.skipped += 1;
                        continue;
                    }
                    let Some(dependency) = self.dependency_path(&package_dir, key, spec) else {
                        // a registry or git dependency: there is no source to load
                        self.skipped += 1;
                        continue;
                    };
                    match self
                        .by_dir
                        .get(&dependency)
                        .and_then(|&p| self.packages[p].lib)
                    {
                        // Cargo's rule: a dependency is called by the dependency's lib name, unless `package = "…"` renames it, and then by the key
                        Some(lib) if dependency != package_dir => {
                            let name = if self.is_renamed(key, spec) {
                                crate_name(key)
                            } else {
                                self.crates[lib].name.clone()
                            };
                            deps.push((name, lib));
                        }
                        // a path to a package that is not there, has no lib, or could not be read
                        _ => self.skipped += 1,
                    }
                }
            }
            let own_lib = self.packages[at].lib;
            let own_name = own_lib.map(|lib| self.crates[lib].name.clone());
            if let Some(lib) = own_lib {
                self.crates[lib].deps = deps.clone();
            }
            for &target in &self.packages[at].targets.clone() {
                let mut target_deps = deps.clone();
                if let (Some(lib), Some(name)) = (own_lib, &own_name) {
                    target_deps.push((name.clone(), lib));
                }
                self.crates[target].deps = target_deps;
            }
        }
    }
}

/// The features on by default: `default = [...]` and what those turn on, by name; and the optional dependencies they enable (`dep:x`, `x/feature`,
/// or the plain name of an optional dependency).
fn default_features(toml: &Table) -> (BTreeSet<String>, BTreeSet<String>) {
    let features = table(toml.get("features"));
    let mut on = BTreeSet::new();
    let mut optional_on = BTreeSet::new();
    let mut queue: Vec<String> = Vec::new();
    if features.is_some_and(|f| f.contains_key("default")) {
        on.insert("default".to_owned());
        queue.extend(strings(features.and_then(|f| f.get("default"))));
    }
    while let Some(item) = queue.pop() {
        if let Some(dependency) = item.strip_prefix("dep:") {
            optional_on.insert(dependency.to_owned());
        } else if let Some((dependency, _feature)) = item.split_once('/') {
            optional_on.insert(dependency.trim_end_matches('?').to_owned());
        } else if features.is_some_and(|f| f.contains_key(&item)) {
            if on.insert(item.clone()) {
                queue.extend(strings(features.and_then(|f| f.get(&item))));
            }
        } else {
            // not a feature of its own: the implicit feature of an optional dependency
            optional_on.insert(item);
        }
    }
    (on, optional_on)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree in memory: `/w/<name>` → text. Folders exist because a file is under them.
    struct Mem(BTreeMap<PathBuf, String>);

    impl Fs for Mem {
        fn read(&self, path: &Path) -> Option<String> {
            self.0.get(path).cloned()
        }

        fn is_file(&self, path: &Path) -> bool {
            self.0.contains_key(path)
        }

        fn list(&self, dir: &Path) -> Vec<String> {
            let mut names: BTreeSet<String> = BTreeSet::new();
            for key in self.0.keys() {
                if let Ok(rest) = key.strip_prefix(dir)
                    && let Some(first) = rest.components().next()
                {
                    names.insert(first.as_os_str().to_string_lossy().into_owned());
                }
            }
            names.into_iter().collect()
        }
    }

    fn mem(files: &[(&str, &str)]) -> Mem {
        Mem(files
            .iter()
            .map(|(path, text)| (Path::new("/w").join(path), (*text).to_owned()))
            .collect())
    }

    /// Reads `/w/<manifest>` of the tree.
    fn read_in(files: &[(&str, &str)], manifest: &str) -> Result<Layout, String> {
        read(&mem(files), &Path::new("/w").join(manifest))
    }

    /// (crate name, root relative to /w, names of its dependencies by crate index's name), in crate order.
    fn shape(layout: &Layout) -> Vec<(String, String, Vec<String>)> {
        layout
            .crates
            .iter()
            .map(|c| {
                let root = paths::relative(Path::new("/w"), &c.root_module).unwrap();
                let deps = c
                    .deps
                    .iter()
                    .map(|(name, at)| format!("{name}={}", layout.crates[*at].name))
                    .collect();
                (c.name.clone(), root, deps)
            })
            .collect()
    }

    fn row(name: &str, root: &str, deps: &[&str]) -> (String, String, Vec<String>) {
        (
            name.to_owned(),
            root.to_owned(),
            deps.iter().map(|d| (*d).to_owned()).collect(),
        )
    }

    const PKG: &str = "[package]\nname = \"my-pkg\"\nedition = \"2021\"\n";

    #[test]
    fn a_lib_is_named_by_the_package_or_by_lib_name_and_path() {
        let layout = read_in(&[("Cargo.toml", PKG), ("src/lib.rs", "")], "Cargo.toml").unwrap();
        assert_eq!(shape(&layout), [row("my_pkg", "src/lib.rs", &[])]);
        assert_eq!(layout.crates[0].edition, "2021");
        assert_eq!(layout.skipped, 0);

        let renamed = format!("{PKG}[lib]\nname = \"core-api\"\npath = \"lib/api.rs\"\n");
        let layout = read_in(
            &[("Cargo.toml", &renamed), ("lib/api.rs", "")],
            "Cargo.toml",
        )
        .unwrap();
        assert_eq!(
            shape(&layout),
            [row("core_api", "lib/api.rs", &[])],
            "[lib] name is read: a wrong name breaks every path call into it"
        );

        // a [lib] whose file is not there is counted, not invented
        let layout = read_in(
            &[("Cargo.toml", &renamed), ("src/main.rs", "")],
            "Cargo.toml",
        )
        .unwrap();
        assert_eq!(
            (layout.skipped, shape(&layout)),
            (1, vec![row("my_pkg", "src/main.rs", &[])])
        );
    }

    #[test]
    fn one_crate_per_target_each_with_the_packages_lib() {
        let files = [
            ("Cargo.toml", PKG),
            ("src/lib.rs", ""),
            ("src/main.rs", ""),
            ("src/bin/a-tool.rs", ""),
            ("src/bin/b/main.rs", ""),
            ("tests/one.rs", ""),
            ("tests/two/main.rs", ""),
            ("tests/two/helper.rs", "not a target"),
        ];
        let layout = read_in(&files, "Cargo.toml").unwrap();
        assert_eq!(
            shape(&layout),
            [
                row("my_pkg", "src/lib.rs", &[]),
                row("my_pkg", "src/main.rs", &["my_pkg=my_pkg"]),
                row("a_tool", "src/bin/a-tool.rs", &["my_pkg=my_pkg"]),
                row("b", "src/bin/b/main.rs", &["my_pkg=my_pkg"]),
                row("one", "tests/one.rs", &["my_pkg=my_pkg"]),
                row("two", "tests/two/main.rs", &["my_pkg=my_pkg"]),
            ]
        );

        // autobins = false: only the [[bin]]s
        let manual = "[package]\nname = \"my-pkg\"\nedition = \"2021\"\nautobins = false\n[[bin]]\nname = \"x\"\npath = \"src/bin/a-tool.rs\"\n";
        let files = [
            ("Cargo.toml", manual),
            ("src/lib.rs", ""),
            ("src/main.rs", ""),
            ("src/bin/a-tool.rs", ""),
        ];
        let layout = read_in(&files, "Cargo.toml").unwrap();
        let names: Vec<_> = layout.crates.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["my_pkg", "x"],
            "main.rs and src/bin are not found when autobins is off"
        );
    }

    #[test]
    fn path_dependencies_dev_dependencies_and_workspace_inheritance() {
        let files = [
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"a\", \"b\"]\n[workspace.dependencies]\nb = { path = \"b\" }\nregistry = \"1\"\n",
            ),
            (
                "a/Cargo.toml",
                "[package]\nname = \"a\"\nedition = \"2021\"\n[dependencies]\nb = { workspace = true }\nrenamed = { path = \"../c\", package = \"c\" }\nserde = \"1\"\n\
                 [dev-dependencies]\nhelper = { path = \"../h\", package = \"h\" }\nregistry = { workspace = true }\n",
            ),
            ("a/src/lib.rs", ""),
            ("a/tests/t.rs", ""),
            (
                "b/Cargo.toml",
                "[package]\nname = \"b\"\nedition = \"2021\"\n",
            ),
            ("b/src/lib.rs", ""),
            (
                "c/Cargo.toml",
                "[package]\nname = \"c\"\nedition = \"2018\"\n",
            ),
            ("c/src/lib.rs", ""),
            (
                "h/Cargo.toml",
                "[package]\nname = \"h\"\nedition = \"2021\"\n[dependencies]\na = { path = \"../a\" }\n",
            ),
            ("h/src/lib.rs", ""),
        ];
        let layout = read_in(&files, "Cargo.toml").unwrap();
        let rows = shape(&layout);
        // a: b (inherited from the workspace), `renamed` (the key is the name in code, the package is c), helper (dev); not serde, not registry
        assert!(
            rows.contains(&row("a", "a/src/lib.rs", &["b=b", "renamed=c", "helper=h"])),
            "{rows:#?}"
        );
        // the test crate has the lib and the same dependencies
        assert!(
            rows.contains(&row(
                "t",
                "a/tests/t.rs",
                &["b=b", "renamed=c", "helper=h", "a=a"]
            )),
            "{rows:#?}"
        );
        // h is no member: a normal dependency of a only through dev, and its own dependency on a (a cycle through a dev-dependency) is kept
        assert!(
            rows.contains(&row("h", "h/src/lib.rs", &["a=a"])),
            "{rows:#?}"
        );
        // c is a path dependency outside the members: a crate of its own, with its own edition
        assert_eq!(
            layout
                .crates
                .iter()
                .find(|c| c.name == "c")
                .unwrap()
                .edition,
            "2018"
        );
        // serde and registry: no source to load, counted
        assert_eq!(layout.skipped, 2);
    }

    #[test]
    fn a_dependency_is_called_by_the_libs_name_unless_it_is_renamed() {
        let files = [
            ("Cargo.toml", "[workspace]\nmembers = [\"a\", \"b\"]\n"),
            (
                "a/Cargo.toml",
                "[package]\nname = \"a\"\nedition = \"2021\"\n[dependencies]\nb = { path = \"../b\" }\nalso_b = { path = \"../b\", package = \"b\" }\n",
            ),
            ("a/src/lib.rs", ""),
            (
                "b/Cargo.toml",
                "[package]\nname = \"b\"\nedition = \"2021\"\n[lib]\nname = \"b_core\"\n",
            ),
            ("b/src/lib.rs", ""),
        ];
        let layout = read_in(&files, "Cargo.toml").unwrap();
        let a = layout.crates.iter().find(|c| c.name == "a").unwrap();
        let named: Vec<_> = a.deps.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            named,
            ["also_b", "b_core"],
            "`b` is called `b_core` in code; the renamed one by its key"
        );
    }

    #[test]
    fn optional_dependencies_follow_the_default_features() {
        let manifest = "[package]\nname = \"p\"\nedition = \"2021\"\n[features]\ndefault = [\"fast\"]\nfast = [\"dep:on\", \"more\"]\nmore = []\nextra = [\"off\"]\n\
                        [dependencies]\non = { path = \"on\", optional = true }\noff = { path = \"off\", optional = true }\n";
        let files = [
            ("Cargo.toml", manifest),
            ("src/lib.rs", ""),
            ("on/Cargo.toml", "[package]\nname = \"on\"\n"),
            ("on/src/lib.rs", ""),
            ("off/Cargo.toml", "[package]\nname = \"off\"\n"),
            ("off/src/lib.rs", ""),
        ];
        let layout = read_in(&files, "Cargo.toml").unwrap();
        let p = layout.crates.iter().find(|c| c.name == "p").unwrap();
        assert_eq!(
            p.deps.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["on"],
            "`off` is not enabled by a default feature"
        );
        // the default features, and what they turn on, are cfgs; `extra` is not on
        let features: Vec<_> = p
            .cfg
            .iter()
            .filter(|c| c.starts_with("feature="))
            .cloned()
            .collect();
        assert_eq!(
            features,
            [
                "feature=\"default\"",
                "feature=\"fast\"",
                "feature=\"more\""
            ]
        );
        assert_eq!(layout.skipped, 1, "the optional dependency that is off");
    }

    #[test]
    fn the_edition_is_read_inherited_or_2015_and_an_unknown_one_skips_the_package() {
        let ws = "[workspace]\nmembers = [\"a\", \"b\", \"c\", \"d\"]\n[workspace.package]\nedition = \"2024\"\n";
        let files = [
            ("Cargo.toml", ws),
            (
                "a/Cargo.toml",
                "[package]\nname = \"a\"\nedition.workspace = true\n",
            ),
            ("a/src/lib.rs", ""),
            ("b/Cargo.toml", "[package]\nname = \"b\"\n"),
            ("b/src/lib.rs", ""),
            (
                "c/Cargo.toml",
                "[package]\nname = \"c\"\nedition = \"2099\"\n",
            ),
            ("c/src/lib.rs", ""),
            (
                "d/Cargo.toml",
                "[package]\nname = \"d\"\nedition = \"2018\"\n",
            ),
            ("d/src/lib.rs", ""),
        ];
        let layout = read_in(&files, "Cargo.toml").unwrap();
        let editions: Vec<_> = layout
            .crates
            .iter()
            .map(|c| (c.name.as_str(), c.edition))
            .collect();
        assert_eq!(editions, [("a", "2024"), ("b", "2015"), ("d", "2018")]);
        assert_eq!(
            layout.skipped, 1,
            "c: an edition nobody knows is not guessed"
        );
    }

    #[test]
    fn the_source_is_the_whole_package_and_the_cfgs_are_the_hosts() {
        let layout = read_in(
            &[
                ("Cargo.toml", PKG),
                ("src/lib.rs", ""),
                ("lib/outside_src.rs", ""),
            ],
            "Cargo.toml",
        )
        .unwrap();
        let c = &layout.crates[0];
        assert_eq!(
            c.package_dir,
            Path::new("/w"),
            "a #[path] module outside src/ is in the package folder"
        );
        assert!(c.cfg.contains(&"debug_assertions".to_owned()));
        assert!(
            c.cfg
                .contains(&format!("target_os=\"{}\"", std::env::consts::OS))
        );
        assert_eq!(c.cfg.contains(&"unix".to_owned()), cfg!(unix));
        assert_eq!(c.cfg.contains(&"windows".to_owned()), cfg!(windows));
        assert!(
            c.cfg
                .contains(&format!("target_pointer_width=\"{}\"", usize::BITS))
        );
        assert_eq!(host_cfgs(), c.cfg, "no features: only the host's");
    }

    #[test]
    fn what_the_reader_does_not_load_is_counted() {
        let ws = "[workspace]\nmembers = [\"p\", \"deep/**\"]\n[patch.crates-io]\nx = { path = \"x\" }\n";
        let package = "[package]\nname = \"p\"\nedition = \"2021\"\n[target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n";
        let files = [
            ("Cargo.toml", ws),
            ("p/Cargo.toml", package),
            ("p/src/lib.rs", ""),
            ("p/build.rs", ""),
            ("p/examples/e1.rs", ""),
            ("p/examples/e2/main.rs", ""),
            ("p/benches/b.rs", ""),
        ];
        let layout = read_in(&files, "Cargo.toml").unwrap();
        // [patch], the `**` member, the target-specific dependencies, build.rs, two examples, one bench
        assert_eq!(layout.skipped, 1 + 1 + 1 + 1 + 2 + 1);
        assert_eq!(shape(&layout), [row("p", "p/src/lib.rs", &[])]);
    }

    #[test]
    fn a_member_finds_its_workspace_above_and_a_manifest_alone_is_a_workspace_of_one() {
        let files = [
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/old\"]\n",
            ),
            (
                "crates/a/Cargo.toml",
                "[package]\nname = \"a\"\nedition = \"2021\"\n",
            ),
            ("crates/a/src/lib.rs", ""),
            (
                "crates/b/Cargo.toml",
                "[package]\nname = \"b\"\nedition = \"2021\"\n",
            ),
            ("crates/b/src/lib.rs", ""),
            ("crates/old/Cargo.toml", "[package]\nname = \"old\"\n"),
            ("crates/old/src/lib.rs", ""),
            ("crates/notes.txt", ""),
        ];
        // from a member's manifest: the whole workspace, as Cargo does; `exclude` leaves one out
        let layout = read_in(&files, "crates/a/Cargo.toml").unwrap();
        let names: Vec<_> = layout.crates.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(layout.workspace_dir, Path::new("/w"));
        assert_eq!(
            layout.manifests,
            [
                Path::new("/w/Cargo.toml"),
                Path::new("/w/crates/a/Cargo.toml"),
                Path::new("/w/crates/b/Cargo.toml")
            ]
        );

        // the workspace above does not list this package: it is on its own
        let alone = [
            ("Cargo.toml", "[workspace]\nmembers = [\"other\"]\n"),
            (
                "pkg/Cargo.toml",
                "[package]\nname = \"pkg\"\nedition = \"2021\"\n",
            ),
            ("pkg/src/lib.rs", ""),
        ];
        let layout = read_in(&alone, "pkg/Cargo.toml").unwrap();
        assert_eq!(shape(&layout), [row("pkg", "pkg/src/lib.rs", &[])]);
        assert_eq!(layout.workspace_dir, Path::new("/w/pkg"));
    }

    #[test]
    fn a_manifest_that_cannot_be_read_is_an_error_with_a_reason() {
        let error = read_in(&[("Cargo.toml", "[package\nname = ")], "Cargo.toml").unwrap_err();
        assert!(
            error.contains("/w/Cargo.toml") || error.contains("Cargo.toml"),
            "{error}"
        );
        assert!(!error.contains('\n'), "one line, for a warning: {error}");
        // valid TOML with nothing to load
        let error =
            read_in(&[("Cargo.toml", "[package]\nname = \"p\"\n")], "Cargo.toml").unwrap_err();
        assert!(error.contains("nothing in it can be loaded"), "{error}");
        // no such file
        assert!(
            read_in(&[], "Cargo.toml")
                .unwrap_err()
                .contains("cannot be read")
        );
    }
}
