//! The book: one entry file, everything it imports, and the tables that say what a name means (docs/08-bend/03 §2, `book_load` and `parse_reso` of
//! `bend.ts`). Bend has no manifest and no toolchain to ask: a project is the file you asked about and what it imports.
//!
//! Nothing here needs a type, so nothing is slow: a book is a few megabytes of text and tables, built in milliseconds. The tables cover the **whole**
//! book, whatever the order of declaration (BD-D12): `bend` refuses a name used before it is declared, we resolve it.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use analyzer_core::edit::MarkerStyle;
use analyzer_core::graph::Stats;
use analyzer_core::outline::{Def, DefKind, Outline};

use crate::base_names;
use crate::paths;
use crate::sem;

/// Where a file is in [`Book::files`]: the order the files were loaded in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileIndex(u32);

impl FileIndex {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A def of the book: its file, and its place in that file's outline. Two defs never share one, even when they share a name (a law and its proof do).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeclId {
    file: FileIndex,
    def: u32,
}

impl DeclId {
    pub(crate) fn new(file: usize, def: usize) -> Self {
        DeclId {
            file: FileIndex(file as u32),
            def: def as u32,
        }
    }

    pub fn file(self) -> FileIndex {
        self.file
    }

    /// The index into the outline of [`DeclId::file`].
    pub fn def(self) -> usize {
        self.def as usize
    }
}

/// What a name means in a file.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Resolved {
    /// A def of the book. For a constructor, the `type` that has it.
    Project(DeclId),
    /// A name of Base's.
    Base,
    /// A name through the alias of a hub import, which is never fetched: the alias.
    Hub(String),
    /// Nothing the book or Base has: a typo, a name `bend` would refuse too, a name through an import whose file is missing.
    Unknown,
}

/// What an `import <path> as <Alias>` points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AliasTarget {
    /// A file of the book. Its own `Book::files` entry says its namespace.
    File(FileIndex),
    /// A package of the hub (`0x<hash>/…`, `<name>@<version>/…`): never fetched, never looked up on disk.
    Hub,
    /// A file that is not there, or cannot be read.
    Missing,
}

/// A file-local name for an imported file: `import ./m.bend as M`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Alias {
    pub name: String,
    pub target: AliasTarget,
}

/// One file of the book.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct FileEntry {
    /// Absolute and real: symlinks are followed, as `bend` does.
    pub path: PathBuf,
    /// The path from the root to the file with `/` for a separator, `..` for each folder above the root: `main.bend`, `lib/x.bend`, `../shared/y.bend`.
    pub relative: String,
    /// The namespace, as Bend computes it: the real path from the **entry file's folder**, without `.bend` (`SPEC` for `./SPEC.bend`, `../lib` for
    /// `../lib.bend`). The entry file's own is `""`, as Bend's is. It is how Bend names things: a def of this file is `ns:name`.
    pub namespace: String,
    /// The file lies under the root. A file imported from outside it is loaded all the same, and counted in `file_outside_root`.
    pub in_root: bool,
    /// What was read (an invalid UTF-8 sequence is replaced, as `bend` does), so every span of `outline` indexes it.
    pub text: String,
    /// The defs of this file, in source order. Its markers are **not** read (no style is known here): ask `bend_syntax::marker` with one.
    pub outline: Outline,
    /// The file's `import <path> as <Alias>` lines, in order. With a repeated alias the first one counts.
    pub aliases: Vec<Alias>,
}

/// One def of one file, with what the book knows of it beyond the outline.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeclEntry {
    pub id: DeclId,
    /// `ns:name` as Bend names it (the bare name when the namespace is `""`). A def that fills the law of an aliased file is named in the law's
    /// namespace: `def Laws.sorted(xs):` in `PROOF.bend`, under `import ./LAWS.bend as Laws`, is `LAWS:sorted`, though it is written in `PROOF.bend`.
    pub qualified: String,
    pub kind: DefKind,
    /// The law this def proves: it has no `->`, and a law of its name is in scope. Then the book's table points at **this** def, not at the law
    /// (a call to the proof is a call to the code); with the law alone, it points at the law.
    pub law: Option<DeclId>,
}

/// What opening a book needs from the disk, and nothing more: this is the loader, as `Project::open` is elsewhere. Everything it returns is owned.
#[derive(Debug)]
pub struct Book {
    root: PathBuf,
    files: Vec<FileEntry>,
    /// Only ever looked up, never iterated (rule 9).
    by_path: HashMap<PathBuf, FileIndex>,
    decls: Vec<DeclEntry>,
    /// For each file, where its first def is in `decls`.
    first_decl: Vec<usize>,
    /// For each file, the names declared **in it** (`tlds` of `bend.ts`, split by file) and the constructors (`ctrs`). Sorted maps, so that nothing here
    /// is ever iterated in a hash's order (rule 9).
    tlds: Vec<BTreeMap<String, DeclId>>,
    ctrs: Vec<BTreeMap<String, DeclId>>,
    imports_base: bool,
    stats: Stats,
}

/// A file being read: its imports are walked one at a time, so the walk keeps its place.
struct Frame {
    path: PathBuf,
    namespace: String,
    text: String,
    outline: Outline,
    facts: sem::Facts,
    next_import: usize,
    aliases: Vec<(String, Raw)>,
}

/// An alias target before every file has a place in the list: files are named by path until then, since a cycle points at a file that is not finished.
enum Raw {
    File(PathBuf),
    Hub,
    Missing,
}

/// Where a file stands in the walk.
enum Mark {
    /// On the stack: an import of it is a cycle.
    Reading,
    /// Done: an import of it is a diamond (two files import one), which is fine.
    Read,
}

/// What an import line is to the loader.
enum ImportKind {
    Base,
    Hub,
    Local,
    /// `import ./m.bend` with no `as`: `bend` refuses it; there is no name to bind.
    NoAlias,
}

fn import_kind(line: &sem::ImportLine) -> ImportKind {
    match &line.alias {
        None if line.path == "Base" => ImportKind::Base,
        None => ImportKind::NoAlias,
        Some(_) if is_hub(&line.path) => ImportKind::Hub,
        Some(_) => ImportKind::Local,
    }
}

/// `0x<hash>/…` or `<name>@<version>/…` (`bend.ts` `book_load`: `hub` and the `NAMED` check). Only the shape is looked at: nothing is fetched.
fn is_hub(path: &str) -> bool {
    let Some((head, _)) = path.split_once('/') else {
        return false;
    };
    let hash = head.strip_prefix("0x").is_some_and(|h| {
        !h.is_empty()
            && h.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    });
    hash || head.contains('@')
}

fn read_text(path: &Path) -> Option<String> {
    Some(String::from_utf8_lossy(&fs::read(path).ok()?).into_owned())
}

/// The namespace of a file: its path from the entry's folder, without `.bend`.
fn namespace(entry_dir: &Path, file: &Path) -> String {
    let relative = paths::relative(entry_dir, file);
    match relative.strip_suffix(".bend") {
        Some(without) => without.to_owned(),
        None => relative,
    }
}

fn qualified(namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        name.to_owned()
    } else {
        format!("{namespace}:{name}")
    }
}

fn count(stats: &mut Stats, key: &str) {
    *stats.language.entry(key.to_owned()).or_default() += 1;
}

fn frame(path: PathBuf, namespace: String, text: String) -> Frame {
    // no marker is read: a book does not know the caller's label (a `docstring` style is one Bend never builds, so `outline` reads none)
    let outline = bend_syntax::outline(&text, MarkerStyle::docstring(""));
    let facts = sem::read(&text);
    Frame {
        path,
        namespace,
        text,
        outline,
        facts,
        next_import: 0,
        aliases: Vec::new(),
    }
}

impl Book {
    /// Opens the book of `entry`, a file under `root` (an absolute path, or one from `root`).
    ///
    /// `root` is the folder that was asked for (D26): a file under it is the project's, and one imported from above or beside it is read all the same
    /// and counted in `file_outside_root`. `None` when the root or the entry is not there, cannot be read, or the entry is not under the root: there is
    /// nothing to load. Nothing else fails: a file that is missing, a cycle, a hub import and a file with a syntax error are counted or kept (see
    /// [`Book::stats`]), and the rest is loaded.
    ///
    /// Nothing is written or fetched, and `~/.bend` is never read.
    pub fn open(root: &Path, entry: &Path) -> Option<Book> {
        let root = paths::real(root)?;
        let entry_path = paths::real(&root.join(entry))?;
        if !entry_path.starts_with(&root) {
            return None;
        }
        let entry_dir = entry_path.parent()?.to_path_buf();
        let mut stats = Stats::default();
        let mut imports_base = false;

        // The files, each after the files it imports: the order `book_load` parses them in, so "the first" of two declarations is `bend`'s first.
        // Rust note: the walk keeps its own stack in a `Vec` instead of calling itself. A chain of imports as long as a project is then no risk to
        // the thread's stack, and the "where was I" of each file is a field (`next_import`).
        let mut done: Vec<Frame> = Vec::new();
        let mut marks: HashMap<PathBuf, Mark> = HashMap::new();
        let text = read_text(&entry_path)?;
        marks.insert(entry_path.clone(), Mark::Reading);
        let mut stack = vec![frame(entry_path, String::new(), text)];
        while let Some(top) = stack.last_mut() {
            let Some(line) = top.facts.imports.get(top.next_import).cloned() else {
                let Some(finished) = stack.pop() else { break };
                marks.insert(finished.path.clone(), Mark::Read);
                done.push(finished);
                continue;
            };
            top.next_import += 1;
            let alias = line.alias.clone().unwrap_or_default();
            match import_kind(&line) {
                ImportKind::Base => imports_base = true,
                ImportKind::NoAlias => count(&mut stats, "import_no_alias"),
                ImportKind::Hub => {
                    count(&mut stats, "hub_import");
                    top.aliases.push((alias, Raw::Hub));
                }
                ImportKind::Local => {
                    let from = top.path.parent().unwrap_or(&entry_dir).to_path_buf();
                    let found = paths::real(&from.join(&line.path)).and_then(|real| {
                        match marks.get(&real) {
                            Some(_) => Some((real, None)),
                            None => read_text(&real).map(|text| (real, Some(text))),
                        }
                    });
                    let Some((real, text)) = found else {
                        count(&mut stats, "import_missing");
                        top.aliases.push((alias, Raw::Missing));
                        continue;
                    };
                    top.aliases.push((alias, Raw::File(real.clone())));
                    match text {
                        // read already: a cycle when it is still being read, a diamond when it is done
                        None => {
                            if matches!(marks.get(&real), Some(Mark::Reading)) {
                                count(&mut stats, "import_cycle");
                            }
                        }
                        Some(text) => {
                            marks.insert(real.clone(), Mark::Reading);
                            let ns = namespace(&entry_dir, &real);
                            stack.push(frame(real, ns, text));
                        }
                    }
                }
            }
        }

        let index: HashMap<PathBuf, FileIndex> = done
            .iter()
            .enumerate()
            .map(|(at, f)| (f.path.clone(), FileIndex(at as u32)))
            .collect();
        let mut facts = Vec::with_capacity(done.len());
        let files: Vec<FileEntry> = done
            .into_iter()
            .map(|f| {
                let in_root = f.path.starts_with(&root);
                if !in_root {
                    count(&mut stats, "file_outside_root");
                }
                facts.push(f.facts);
                FileEntry {
                    relative: paths::relative(&root, &f.path),
                    aliases: f
                        .aliases
                        .into_iter()
                        .map(|(name, raw)| Alias {
                            name,
                            target: match raw {
                                Raw::File(path) => index
                                    .get(&path)
                                    .map_or(AliasTarget::Missing, |&i| AliasTarget::File(i)),
                                Raw::Hub => AliasTarget::Hub,
                                Raw::Missing => AliasTarget::Missing,
                            },
                        })
                        .collect(),
                    path: f.path,
                    namespace: f.namespace,
                    in_root,
                    text: f.text,
                    outline: f.outline,
                }
            })
            .collect();

        let mut book = Book {
            root,
            files,
            by_path: index,
            decls: Vec::new(),
            first_decl: Vec::new(),
            tlds: Vec::new(),
            ctrs: Vec::new(),
            imports_base,
            stats,
        };
        book.build_tables(&facts);
        Some(book)
    }

    /// The tables, over the whole book. Two passes, so that the order of the files and of the declarations does not matter: first every declaration
    /// that is not a proof, then each proof finds its law.
    fn build_tables(&mut self, facts: &[sem::Facts]) {
        let mut proofs: Vec<DeclId> = Vec::new();
        self.tlds = vec![BTreeMap::new(); self.files.len()];
        self.ctrs = vec![BTreeMap::new(); self.files.len()];
        for (f, file_facts) in facts.iter().enumerate() {
            self.first_decl.push(self.decls.len());
            // Rust note: the loop counts files and takes what it needs out of each def, rather than holding a borrow of `self.files` while `declare`
            // borrows `self` mutably: the compiler refuses two borrows of one value when one of them writes.
            for i in 0..self.files[f].outline.defs.len() {
                let id = DeclId::new(f, i);
                let (name, name_range, kind) = {
                    let def = &self.files[f].outline.defs[i];
                    (def.qualname.clone(), def.name_range, def.kind.clone())
                };
                self.decls.push(DeclEntry {
                    id,
                    qualified: qualified(&self.files[f].namespace, &name),
                    kind: kind.clone(),
                    law: None,
                });
                let fact = file_facts.of(name_range);
                if fact.is_some_and(|d| d.fills_law) && matches!(kind, DefKind::Function) {
                    proofs.push(id);
                    continue;
                }
                self.declare(f, &name, id);
                for ctor in fact.iter().flat_map(|d| &d.ctors) {
                    self.declare_ctor(f, ctor, id);
                }
            }
        }
        for id in proofs {
            self.fill_law(id);
        }
    }

    /// `name` is declared in file `f` by `id`. The first declaration of a name keeps it: a second one, in this file or another, or a name Base has
    /// (`bend` refuses all three, `parse_fresh`) is counted and loses.
    fn declare(&mut self, f: usize, name: &str, id: DeclId) {
        let base = self.imports_base && base_names::TLDS.binary_search(&name).is_ok();
        let table = &mut self.tlds[f];
        if base || table.contains_key(name) {
            count(&mut self.stats, "duplicate_declaration");
        }
        // Rust note: `entry(..).or_insert(..)` is "insert unless it is there": the first one stays.
        table.entry(name.to_owned()).or_insert(id);
    }

    fn declare_ctor(&mut self, f: usize, name: &str, ty: DeclId) {
        let base = self.imports_base && base_names::CTRS.binary_search(&name).is_ok();
        let table = &mut self.ctrs[f];
        if base || table.contains_key(name) {
            count(&mut self.stats, "duplicate_declaration");
        }
        table.entry(name.to_owned()).or_insert(ty);
    }

    /// `def f(x):` with no `->` fills the law `f` of the file, or `def M.f(x):` the law `f` of the file `M` names (`parse_def`: `parse_reso` of the
    /// name, then the tld found there). The table then points at the proof. With no law there to fill, it is declared like any def.
    fn fill_law(&mut self, id: DeclId) {
        let file = &self.files[id.file().index()];
        let name = file.outline.defs[id.def()].qualname.clone();
        let (target, bare) = match name.split_once('.') {
            Some((prefix, rest)) => match file.aliases.iter().find(|a| a.name == prefix) {
                Some(Alias {
                    target: AliasTarget::File(t),
                    ..
                }) => (*t, rest.to_owned()),
                _ => (id.file(), name.clone()),
            },
            None => (id.file(), name.clone()),
        };
        let held = self.tlds[target.index()].get(&bare).copied();
        match held {
            None => self.declare(id.file().index(), &name, id),
            Some(law) => {
                let at = self.slot(id);
                self.decls[at].qualified = qualified(&self.files[target.index()].namespace, &bare);
                if matches!(&self.decls[self.slot(law)].kind, DefKind::Other(kind) if kind == "law")
                {
                    self.decls[at].law = Some(law);
                    self.tlds[target.index()].insert(bare, id);
                } else {
                    // the name is a def or a type already, or a proof filled the law before this one: a second declaration of it
                    count(&mut self.stats, "duplicate_declaration");
                }
            }
        }
    }

    fn slot(&self, id: DeclId) -> usize {
        self.first_decl[id.file().index()] + id.def()
    }

    /// The root: the folder that was asked for, absolute and real.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The files in load order: each one **after** the files it imports (the order `bend` parses them in), so the entry file is the last.
    pub fn files(&self) -> &[FileEntry] {
        &self.files
    }

    /// The entry file.
    pub fn entry(&self) -> FileIndex {
        FileIndex(self.files.len().saturating_sub(1) as u32)
    }

    pub fn file(&self, file: FileIndex) -> Option<&FileEntry> {
        self.files.get(file.index())
    }

    /// The file at `path`, absolute or from the root, symlinks followed. `None` when no file of the book is there.
    pub fn file_at(&self, path: &Path) -> Option<FileIndex> {
        self.by_path
            .get(&paths::real(&self.root.join(path))?)
            .copied()
    }

    /// Every def of every file: the files in load order, the defs of each in source order.
    pub fn decls(&self) -> &[DeclEntry] {
        &self.decls
    }

    pub fn decl(&self, id: DeclId) -> Option<&DeclEntry> {
        let first = *self.first_decl.get(id.file().index())?;
        let last = self
            .first_decl
            .get(id.file().index() + 1)
            .copied()
            .unwrap_or(self.decls.len());
        (first + id.def() < last).then(|| &self.decls[first + id.def()])
    }

    /// The outline's def.
    pub fn def(&self, id: DeclId) -> Option<&Def> {
        self.files
            .get(id.file().index())?
            .outline
            .defs
            .get(id.def())
    }

    /// Some file of the book wrote `import Base`. Base's names mean something only then, as in `bend`, where Base is in the book or is not.
    pub fn imports_base(&self) -> bool {
        self.imports_base
    }

    /// What the book counted and did not follow or could not honour, by name, in `Stats::language`: `import_cycle`, `import_missing`, `hub_import`
    /// (once per import line), `import_no_alias`, `file_outside_root`, `duplicate_declaration`. A counter that is zero is not there.
    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// What `name`, as written in `file`, means, the last three steps of `parse_reso`: an alias (`M.x` is the `x` of the file `M` names), the file's own
    /// def, Base's. The first, a local binder, is the walk's: this takes a name that is not bound. A law and its proof are one name, and it means the
    /// proof (the law alone when there is no proof).
    pub fn resolve(&self, file: FileIndex, name: &str) -> Resolved {
        self.reso(file, name, &self.tlds, base_names::TLDS)
    }

    /// What `name` means when `bend` makes the name up itself (`parse_own`: `Array.get` for `a[i]`, `T.bind` for a do block): this file's own def, else Base's,
    /// and no alias is looked at.
    pub fn resolve_own(&self, file: FileIndex, name: &str) -> Resolved {
        let own = self.tlds.get(file.index()).and_then(|t| t.get(name));
        match own {
            Some(&id) => Resolved::Project(id),
            None if self.imports_base && base_names::TLDS.binary_search(&name).is_ok() => {
                Resolved::Base
            }
            None => Resolved::Unknown,
        }
    }

    /// The same for a constructor (`K{..}`, a pattern): a project constructor resolves to the def of the `type` that has it.
    pub fn ctor(&self, file: FileIndex, name: &str) -> Resolved {
        self.reso(file, name, &self.ctrs, base_names::CTRS)
    }

    fn reso(
        &self,
        file: FileIndex,
        name: &str,
        table: &[BTreeMap<String, DeclId>],
        base: &[&str],
    ) -> Resolved {
        let (Some(entry), Some(own)) = (self.files.get(file.index()), table.get(file.index()))
        else {
            return Resolved::Unknown;
        };
        let base = || {
            if self.imports_base && base.binary_search(&name).is_ok() {
                Resolved::Base
            } else {
                Resolved::Unknown
            }
        };
        if let Some((prefix, rest)) = name.split_once('.')
            && let Some(alias) = entry.aliases.iter().find(|a| a.name == prefix)
        {
            match alias.target {
                AliasTarget::Hub => return Resolved::Hub(alias.name.clone()),
                AliasTarget::File(target) => {
                    if let Some(&id) = table.get(target.index()).and_then(|t| t.get(rest)) {
                        return Resolved::Project(id);
                    }
                }
                AliasTarget::Missing => {}
            }
            // the file does not have it: `bend` then tries the whole name as Base's. A def of this file written with the alias's name (`bend` refuses
            // the declaration) is still found, rather than called unknown
            return match own.get(name) {
                Some(&id) => Resolved::Project(id),
                None => base(),
            };
        }
        match own.get(name) {
            Some(&id) => Resolved::Project(id),
            None => base(),
        }
    }

    /// The names declared in the book as `ns:name`, with the def each means, sorted: Bend's `tlds` table. A law is there as its proof when it has one.
    pub fn declared(&self) -> Vec<(String, DeclId)> {
        self.listed(&self.tlds)
    }

    /// The constructors as `ns:Name`, with the def of the `type` of each, sorted: Bend's `ctrs` table.
    pub fn constructors(&self) -> Vec<(String, DeclId)> {
        self.listed(&self.ctrs)
    }

    fn listed(&self, table: &[BTreeMap<String, DeclId>]) -> Vec<(String, DeclId)> {
        let mut all: Vec<(String, DeclId)> = table
            .iter()
            .zip(&self.files)
            .flat_map(|(names, file)| {
                names
                    .iter()
                    .map(|(name, id)| (qualified(&file.namespace, name), *id))
            })
            .collect();
        all.sort();
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `binary_search` needs the lists sorted bytewise and without a repeat: a generated list that is not would make `resolve` miss names.
    #[test]
    fn base_lists_are_sorted_and_each_name_is_once() {
        for list in [base_names::TLDS, base_names::CTRS] {
            assert!(list.windows(2).all(|w| w[0] < w[1]));
        }
        assert!(base_names::TLDS.binary_search(&"Nat.add").is_ok());
        assert!(base_names::TLDS.binary_search(&"IO.print").is_ok());
        assert!(base_names::CTRS.binary_search(&"Some").is_ok());
        // a law and the def that proves it are one name; 517 names, 50 constructors (scripts/bend-base-names.sh)
        assert_eq!(base_names::TLDS.len(), 517);
        assert_eq!(base_names::CTRS.len(), 50);
    }

    #[test]
    fn a_hub_path_is_a_hash_or_a_name_at_a_version_then_a_slash() {
        assert!(is_hub("0x00ab12cd/main.bend"));
        assert!(is_hub("util@1.0.0.0/lib.bend"));
        assert!(!is_hub("./lib.bend"));
        assert!(!is_hub("../lib.bend"));
        assert!(!is_hub("0x/main.bend"));
        assert!(!is_hub("0xZZ/main.bend"));
        assert!(!is_hub("lib.bend"));
        assert!(!is_hub("sub/lib.bend"));
    }
}
