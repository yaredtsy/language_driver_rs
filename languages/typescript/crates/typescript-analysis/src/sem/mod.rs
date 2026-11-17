//! The only file that imports the engine (`tsrs_*`). Everything else asks `Sem` and gets plain data.
//!
//! A `Sem` is one loaded program: the tsconfig that owns a file, every file it pulls in, parsed and bound. It follows `tsrs_lint`'s
//! `engine.rs` (`create_program`) and the memory pattern measured in S-TS3 (`ENGINE.md`): build inside one region, give the loaders
//! a region set, and free the program on drop with that region entered. The region alone is not enough: the `Program` and its processed
//! files are `Box::leak`ed on the heap, outside it (60 opens grew RSS to 349 MB without the free, 24 MB with it: `tests/memory.rs`).

#![cfg_attr(
    not(feature = "__proof"),
    expect(
        unreachable_pub,
        reason = "`Sem` and `File` are `pub` only for `proof`, which exists with the feature `__proof`"
    )
)]

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tsrs_ast::{self as ast, Kind, Node, NodeFlags, SourceFile};
use tsrs_checker::Checker;
use tsrs_compiler::{CheckerHandle, Context, Program, ProgramOptions, ProgramRegions};
use tsrs_core::arena::Region;
use tsrs_core::{
    CompilerOptions, JsxEmit, ModuleKind, ModuleResolutionKind, P, ScriptTarget, Tristate, tspath,
};
use tsrs_tsoptions::{ParseConfigHost, ParsedCommandLine};
use tsrs_vfs::{FS, bundled, cachedvfs, osvfs};

use typescript_syntax::{Def, DefKind, Dialect, MarkerStyle, Outline, Placement, Span};

use crate::OpenError;

mod collect;
#[cfg(test)]
mod coverage;
mod handed;
mod hierarchy;
mod project;
/// What a site calls, and what a def is (TS4 step 3). The resolver is a port of `tsrs_ls`'s `resolve_call_hierarchy_declaration` (`callhierarchy.rs`, a
/// private module there, so it cannot be called: S-TS3 in `ENGINE.md`), which is what "follow the engine" (D7) means for a call: from the callee
/// expression, a declaration that is itself callable, else the declaration of the symbol the checker finds there, once.
mod resolve;
#[cfg(test)]
mod tests;
mod uncovered;

use collect::{Body, Collector};
pub(crate) use handed::Handed;
use project::{
    INFERRED, Owner, find_owner, inferred_config, place_between, relative_to, to_engine_path,
    under_root,
};
pub(crate) use resolve::{Init, Resolved};
pub(crate) use uncovered::Uncovered;

/// First chunk of the region a program lives in: the 1 MB `tsrs_lint` uses for one program.
const CHUNK: usize = 1 << 20;

/// Where tsrs reads config files from: the real disk, with the bundled `lib.*.d.ts` mounted.
struct ConfigHost {
    fs: Arc<dyn FS>,
    cwd: String,
}

impl ParseConfigHost for ConfigHost {
    fn fs(&self) -> &dyn FS {
        &*self.fs
    }
    fn get_current_directory(&self) -> &str {
        &self.cwd
    }
}

/// A source file of a `Sem`'s program. tsrs's `P<SourceFile>` is `'static` and the compiler cannot see the program being freed, so this adds a
/// lifetime: a `File<'p>` can only come from a `&'p Sem` (`PhantomData<&'p Sem>` stores nothing and borrows the `Sem`), and `Sem::drop`'s
/// `unsafe` rests on that. `lib.rs`'s `proof` module has the compile-fail proofs: it cannot outlive its `Sem`, and it cannot cross threads.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct File<'p> {
    node: P<SourceFile>,
    _sem: PhantomData<&'p Sem>,
    /// A raw pointer is neither `Send` nor `Sync`, so neither is `File`: a handle stays on the thread that asked for it (`proof` pins this).
    _one_thread: PhantomData<*const ()>,
}

/// One file's outline, and for each def the node of the **program's** tree it names. The outline is made from the file's text by
/// `typescript_syntax::outline_with_nodes`, which also says where each def's node is (a byte span); the node is the one in the program's tree with that
/// span and a kind that can be a def's node. The same text parses to the same spans, so the two trees agree, and the rule for what a def is is written
/// once, in `typescript-syntax`. `tests` proves the map is one to one on every fixture of the outline suite.
pub(crate) struct FileDefs {
    pub(crate) outline: Outline,
    /// The node of each def, in the order of `outline.defs`. `None` when no node, or more than one, has the span: a bug in one of the two parses,
    /// which `describe` turns into an `Unresolved` edge and a count (rule 8).
    pub(crate) nodes: Vec<Option<P<Node>>>,
    /// The def a node is, for the nodes in `nodes`: the way back, and how the walk knows a nested node is a def of its own.
    pub(crate) def_of: HashMap<P<Node>, usize>,
    /// For each def of `outline.defs`: whether its id is its marker. The first def of the file, in source order, with a marker text owns it; a later def with
    /// the same text is named by where it is (D34), so a def's id is the same in every request, whatever else the request reaches. `false` for a def with no
    /// marker.
    pub(crate) owns_marker: Vec<bool>,
}

/// The defs of one file made so far, one per marker style (a marker is read by its label, so another label is another outline). A file is read in
/// one or two styles, so a short list compared in place is cheaper than a key that must be built for every lookup.
struct Cached {
    label: String,
    placement: Placement,
    defs: Arc<FileDefs>,
}

/// The files' defs made so far, by file.
type FileDefsCache = HashMap<P<SourceFile>, Vec<Cached>>;

/// A def of an outline, read in place: it holds the outline alive and points into it, so reading its qualname, kind or range copies nothing.
pub(crate) struct DefRef {
    defs: Arc<FileDefs>,
    index: usize,
}

impl std::ops::Deref for DefRef {
    type Target = Def;

    fn deref(&self) -> &Def {
        &self.defs.outline.defs[self.index]
    }
}

/// The node kinds `outline_with_nodes` can name: what has a body, a class, or a declaration of a def with no body.
fn can_be_a_def(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::FunctionDeclaration
            | Kind::FunctionExpression
            | Kind::ArrowFunction
            | Kind::ClassDeclaration
            | Kind::ClassExpression
            | Kind::MethodDeclaration
            | Kind::Constructor
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::InterfaceDeclaration
            | Kind::MethodSignature
            | Kind::EnumDeclaration
            | Kind::TypeAliasDeclaration
            | Kind::ModuleDeclaration
    )
}

/// The nodes of `file` that can be a def's node and whose span is one of `wanted`, by span. The span is the node's own bytes, from its first token
/// to its end (tsrs's `pos` includes the trivia before it, so the start is moved past it), as `typescript-syntax` measures it.
fn nodes_by_span(file: P<SourceFile>, wanted: &HashSet<Span>) -> HashMap<Span, Vec<P<Node>>> {
    let text = file.text();
    let mut found: HashMap<Span, Vec<P<Node>>> = HashMap::new();
    let mut pending = vec![file.as_node()];
    while let Some(node) = pending.pop() {
        if can_be_a_def(node.kind()) {
            let span = own_span(text, node);
            if wanted.contains(&span) {
                found.entry(span).or_default().push(node);
            }
        }
        node.for_each_child(&mut |child| {
            pending.push(child);
            false
        });
    }
    found
}

/// A def of one file's outline (`typescript_syntax::outline`, the rule TS1 froze), or the file's own code (the module, D32): a `File` and which of the two. It
/// holds no tsrs pointer of its own, only the `File`, so it is valid for the `Sem`'s life and no longer, stays on one thread, and is not shared, exactly as
/// `File` is. `proof` pins all three for `Func` too.
///
/// The module has no row in the outline (rule 10: the outline does not change), so there is no index to give it. `at` is private and its type is an enum,
/// so a place that needs the outline's row must say what it does for the module: it asks `outline_index`, which is an `Option`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Func<'p> {
    file: File<'p>,
    at: At,
}

/// What a `Func` is in its file.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum At {
    /// The file's own code: its top-level statements, down to the next def.
    Module,
    /// The def at this index of the file's outline.
    Def(usize),
}

impl<'p> Func<'p> {
    /// The def at `index` of the outline of `file`.
    pub(crate) fn def(file: File<'p>, index: usize) -> Self {
        Func {
            file,
            at: At::Def(index),
        }
    }

    /// The file's own code.
    pub(crate) fn module(file: File<'p>) -> Self {
        Func {
            file,
            at: At::Module,
        }
    }

    /// The index in the file's outline; `None` for the module, which has no row there.
    pub(crate) fn outline_index(self) -> Option<usize> {
        match self.at {
            At::Def(index) => Some(index),
            At::Module => None,
        }
    }
}

pub struct Sem {
    root: PathBuf,
    /// The program is freed by `Drop`. A `P<…>` taken from it must not outlive `self`.
    program: &'static Program,
    /// Every config file that fed this program: the ones the owner search read (a broken one too) and the ones they extend. Relative to the root
    /// where they are under it, else as the engine spells them. Sorted.
    config_files: Vec<String>,
    /// The config whose program this is (relative to the root), or `None` for an inferred project.
    config_file: Option<String>,
    /// `Cell` is `Send` but not `Sync`, so neither is a `Sem` shared by reference: one checker answers one question at a time, and nothing
    /// here makes concurrent questions safe (`proof` pins this). Moving a `Sem` to another thread stays allowed.
    _not_shared: PhantomData<Cell<()>>,
    /// Whether a checker handle is out (`Sem::checker`).
    checker_held: Cell<bool>,
    /// One outline per (file, marker label), made on first use. A `Sem` is not `Sync`, so a `RefCell` is enough.
    file_defs: RefCell<FileDefsCache>,
    /// What the walk of each def's body found, made on first use, by the def's node: its sites, the defs it stops at, the references by name.
    bodies: RefCell<HashMap<P<Node>, Arc<Body>>>,
    // Fields drop in this order, after `Drop::drop` has freed the program: the loaders' regions, then the program's own.
    _loaders: Arc<ProgramRegions>,
    region: Region,
}

impl Sem {
    /// Loads the program of the `tsconfig.json` (or `jsconfig.json`) nearest above `file`, looking no further up than `root`; with none, an
    /// inferred project built from `file` alone.
    /// `file` is relative to `root`, or absolute, and must be under `root` once `..` is resolved (`OpenError::OutsideRoot` otherwise: D33). A program that loads
    /// with type errors is not an error.
    pub fn open(root: &Path, file: &Path) -> Result<Sem, OpenError> {
        let root = root.canonicalize().map_err(|_| OpenError::NoSuchFile)?;
        let file = root
            .join(file)
            .canonicalize()
            .map_err(|_| OpenError::NoSuchFile)?;
        if !file.is_file() {
            return Err(OpenError::NoSuchFile);
        }
        // `join` and `canonicalize` have resolved any `..`; the rule is the analysis's own (`relative_path`), so open and the walk agree on "under"
        if under_root(&to_engine_path(&root), &to_engine_path(&file)).is_none() {
            return Err(OpenError::OutsideRoot);
        }
        let cwd = to_engine_path(&root);
        let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(cachedvfs::from(osvfs::fs())));
        let config_host = ConfigHost {
            fs: Arc::clone(&fs),
            cwd: cwd.clone(),
        };

        let region = Region::new_scratch(CHUNK);
        let loaders = Arc::new(ProgramRegions::default());
        let mut config_inputs = BTreeSet::new();
        let (program, config_name) = {
            let _scope = region.enter();
            let mut skipped = Vec::new();
            let owner = find_owner(&root, &file, &config_host, &mut config_inputs, &mut skipped)?;
            let (config_name, mut config) = match owner {
                Owner::Config(name, config) => (Some(name), *config),
                Owner::NoneHolds(name) => {
                    return Err(OpenError::NotInProject {
                        config: relative_to(&root, &name),
                        skipped,
                    });
                }
                Owner::NoConfig => (None, inferred_config(&file, &config_host)),
            };
            // One checker answers every question, so the same project gives the same answers in the same order (rule 9). Not
            // `single_threaded`: that also loads files one at a time, 3.5 times slower on a 1,500-file program (ENGINE.md).
            let compiler_options = config
                .compiler_options()
                .map(|options| (*options).clone())
                .unwrap_or_default();
            config.parsed_config.compiler_options = Some(P::new(CompilerOptions {
                checkers: Some(1),
                ..compiler_options
            }));
            let host = tsrs_compiler::new_compiler_host(
                &cwd,
                fs,
                &bundled::default_lib_path(),
                None,
                None,
                None,
            );
            let mut options = ProgramOptions::new(P::new(config), host);
            // A referenced project's source stands in for its build output (its `.d.ts`, often not built): as `tsrs_lint` does for a config.
            options.use_source_of_project_reference = config_name.is_some();
            // The checkers' memory goes back with the program (S-TS3).
            options.checker_regions = true;
            options.regions = Some(Arc::clone(&loaders));
            let program = tsrs_compiler::new_program(options);
            // Load and bind only: calls are resolved on demand, so nothing here type-checks the whole program.
            program.bind_source_files();
            (program, config_name)
        };

        let config_files = config_inputs
            .iter()
            .map(|name| relative_to(&root, name))
            .collect();
        let config_file = config_name.as_ref().map(|name| relative_to(&root, name));
        let sem = Sem {
            root,
            program,
            config_files,
            config_file,
            _not_shared: PhantomData,
            checker_held: Cell::new(false),
            file_defs: RefCell::new(HashMap::new()),
            bodies: RefCell::new(HashMap::new()),
            _loaders: loaders,
            region,
        };
        if sem
            .program
            .get_source_file(&to_engine_path(&file))
            .is_none()
        {
            return Err(OpenError::NotInProject {
                config: match &config_name {
                    Some(name) => relative_to(&sem.root, name),
                    None => INFERRED.into(),
                },
                skipped: Vec::new(),
            });
        }
        Ok(sem)
    }

    /// Every file of the program: the project's, `node_modules`, and the bundled libraries. Ask `is_project_code` to tell them apart.
    pub(crate) fn files(&self) -> impl Iterator<Item = File<'_>> {
        self.program.get_source_files().iter().map(|&node| File {
            node,
            _sem: PhantomData,
            _one_thread: PhantomData,
        })
    }

    /// Every function, method, class and namespace of the project's own files that can be walked (it has a node in the program's tree), in the program's file order
    /// and source order: what a measurement over the whole project starts from.
    #[cfg(test)]
    pub(crate) fn project_defs(&self, style: MarkerStyle<'_>) -> Vec<Func<'_>> {
        let mut found = Vec::new();
        for file in self.files() {
            if !self.is_project_code(file) {
                continue;
            }
            let Some(defs) = self.file_defs(file, style) else {
                continue;
            };
            for (index, def) in defs.outline.defs.iter().enumerate() {
                if matches!(
                    def.kind,
                    typescript_syntax::DefKind::Function
                        | typescript_syntax::DefKind::Method
                        | typescript_syntax::DefKind::Class
                        | typescript_syntax::DefKind::Namespace
                ) && defs.nodes[index].is_some()
                {
                    found.push(Func::def(file, index));
                }
            }
        }
        found
    }

    /// The file's own code (the module, D32) of every project file, in the program's file order: what a measurement that also enters each file starts from.
    #[cfg(test)]
    pub(crate) fn project_modules(&self) -> Vec<Func<'_>> {
        self.files()
            .filter(|&file| self.is_project_code(file))
            .map(Func::module)
            .collect()
    }

    /// The file at `relative` (to the root, with `/`; `./` and `..` are folded), if the program holds it.
    pub fn file(&self, relative: &str) -> Option<File<'_>> {
        let name = tspath::get_normalized_absolute_path(relative, &to_engine_path(&self.root));
        let node = self.program.get_source_file(&name)?;
        Some(File {
            node,
            _sem: PhantomData,
            _one_thread: PhantomData,
        })
    }

    /// The defs of a TypeScript or JavaScript file: its outline and each def's node in the program's tree (see `FileDefs`). Made once per (file,
    /// label). `None` for a file that is not code (an imported `.json`).
    pub(crate) fn file_defs(
        &self,
        file: File<'_>,
        style: MarkerStyle<'_>,
    ) -> Option<Arc<FileDefs>> {
        let dialect = Dialect::from_path(file.node.file_name())?;
        if let Some(found) = self.file_defs.borrow().get(&file.node).and_then(|made| {
            made.iter()
                .find(|c| c.label == style.label && c.placement == style.placement)
        }) {
            return Some(Arc::clone(&found.defs));
        }
        // looking at the tree may allocate in the engine; the program's region is where that belongs
        let _scope = self.region.enter();
        let (outline, spans) =
            typescript_syntax::outline_with_nodes(file.node.text(), dialect, style);
        let by_span = nodes_by_span(file.node, &spans.iter().copied().collect());
        let nodes: Vec<Option<P<Node>>> = spans
            .iter()
            .map(|span| match by_span.get(span).map(Vec::as_slice) {
                Some([only]) => Some(*only),
                _ => None,
            })
            .collect();
        let def_of = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| Some((((*node)?), index)))
            .collect();
        let owns_marker = marker_owners(&outline);
        let made = Arc::new(FileDefs {
            outline,
            nodes,
            def_of,
            owns_marker,
        });
        self.file_defs
            .borrow_mut()
            .entry(file.node)
            .or_default()
            .push(Cached {
                label: style.label.to_owned(),
                placement: style.placement,
                defs: Arc::clone(&made),
            });
        Some(made)
    }

    /// Every def of `file` with this qualname, in source order, as `Project::defs_named` reports them.
    pub(crate) fn defs_named(
        &self,
        file: &str,
        qualname: &str,
        style: MarkerStyle<'_>,
    ) -> Option<Vec<crate::DefMatch>> {
        let file = self.file(file)?;
        // the defs a person may choose from are the ones that can be entries (`entry`): project code only (D33)
        if !self.is_project_code(file) {
            return None;
        }
        let defs = self.file_defs(file, style)?;
        Some(
            defs.outline
                .defs
                .iter()
                .enumerate()
                .filter(|(_, def)| def.qualname == qualname)
                .enumerate()
                .map(|(occurrence, (index, def))| crate::DefMatch {
                    occurrence: occurrence as u16,
                    kind: def.kind.clone(),
                    range: def.range,
                    marker: def.marker.clone(),
                    owns_marker: def.marker.is_some() && defs.owns_marker[index],
                })
                .collect(),
        )
    }

    /// A test hook: forget the program's node for one def, as if the two parses had disagreed about it. This is how `def_without_node` runs; nothing in
    /// the suite can make the real parses disagree.
    #[cfg(test)]
    pub(crate) fn forget_node_of(&self, func: Func<'_>, style: MarkerStyle<'_>) {
        let defs = self
            .file_defs(func.file, style)
            .expect("a Func has an outline");
        let mut nodes = defs.nodes.clone();
        nodes[func.outline_index().expect("a def, not the module")] = None;
        let def_of = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| Some(((*node)?, index)))
            .collect();
        let replaced = Arc::new(FileDefs {
            outline: defs.outline.clone(),
            nodes,
            def_of,
            owns_marker: defs.owns_marker.clone(),
        });
        for cached in self
            .file_defs
            .borrow_mut()
            .get_mut(&func.file.node)
            .expect("cached just now")
        {
            if cached.label == style.label && cached.placement == style.placement {
                cached.defs = Arc::clone(&replaced);
            }
        }
    }

    /// The `occurrence`th def (0-based, in source order: the number `StableDef::Path::nth` holds) with this `qualname` in the file at `file`
    /// (relative to the root). A qualname is not unique in a file (overloads, a getter and a setter), so the caller says which.
    pub fn entry(
        &self,
        file: &str,
        qualname: &str,
        occurrence: u16,
        style: MarkerStyle<'_>,
    ) -> Option<Func<'_>> {
        let file = self.file(file)?;
        // a file outside the root, or in `node_modules`, has no entry (D33): `describe` takes every `Func` to be under the root
        if !self.is_project_code(file) {
            return None;
        }
        let defs = self.file_defs(file, style)?;
        let (def, _) = defs
            .outline
            .defs
            .iter()
            .enumerate()
            .filter(|(_, def)| def.qualname == qualname)
            .nth(usize::from(occurrence))?;
        Some(Func::def(file, def))
    }

    /// The file's own code at `file` (relative to the root), as an entry (D32). Like `entry`, only project code has one: a file outside the root, in
    /// `node_modules`, a declaration file or a file that is not code answers `None` (D33).
    pub fn file_entry(&self, file: &str) -> Option<Func<'_>> {
        let file = self.file(file)?;
        self.is_project_code(file).then(|| Func::module(file))
    }

    /// The plain calls in the body of `func`: the callee expression of each, as an absolute byte span in the file, in source order (by where the
    /// callee starts, then where it ends). Walks the **program's** tree from the def's node, through `FileDefs`. `None` for a def with no node (see
    /// `FileDefs::nodes`).
    pub(crate) fn call_sites(&self, func: Func<'_>, style: MarkerStyle<'_>) -> Option<Vec<Span>> {
        let body = self.body(func, style)?;
        Some(body.sites.iter().map(|call| call.span).collect())
    }

    /// What reading `func`'s body found: its sites with their nodes, sorted by span (what `call_sites` reports and `callee` asks the checker about), the
    /// defs the walk stopped at, and the references by name. One pass, made once per def.
    fn body(&self, func: Func<'_>, style: MarkerStyle<'_>) -> Option<Arc<Body>> {
        let defs = self.file_defs(func.file, style)?;
        let node = self.node_of(func, style)?;
        if let Some(found) = self.bodies.borrow().get(&node) {
            return Some(Arc::clone(found));
        }
        let _scope = self.region.enter();
        let mut collector = Collector::new(func.file.node.text(), &defs.def_of, node);
        collector.body_of(node);
        let accessors = self.accessor_sites(&collector.accesses, collector.text);
        collector.sites.extend(accessors);
        collector.sites.sort_by_key(|call| call.span);
        let made = Arc::new(Body {
            sites: collector.sites,
            nested: collector.nested,
            lits: collector.lits,
            elems: collector.elems,
            binds: collector.binds,
            call_apply: collector.call_apply,
            refs: collector.refs,
            destructured: collector.destructured,
            assigns: collector.assigns,
        });
        self.bodies.borrow_mut().insert(node, Arc::clone(&made));
        Some(made)
    }

    /// The class's own `constructor`, a def of the same file whose parent is the class. `None` when the class declares none, or when `class` is not a
    /// class. What a construction runs is `init_chain`, which also looks at the base classes.
    pub(crate) fn constructor_of<'p>(
        &'p self,
        class: Func<'p>,
        style: MarkerStyle<'_>,
    ) -> Option<Func<'p>> {
        let defs = self.file_defs(class.file, style)?;
        let class_index = class.outline_index()?;
        // Overloads are one def each (two signatures and the implementation): the one that runs is the one with a body, and a signature has nothing to
        // walk, so choosing the first would lose every call of the constructor.
        let mut candidates = defs
            .outline
            .defs
            .iter()
            .enumerate()
            .filter(|(_, def)| def.parent == Some(class_index) && def.name == "constructor")
            .map(|(index, _)| index);
        let first = candidates.next()?;
        let index = std::iter::once(first)
            .chain(candidates)
            .find(|&index| defs.nodes[index].is_some_and(|node| node.body().is_some()))
            .unwrap_or(first);
        Some(Func::def(class.file, index))
    }

    /// What the outline says the def is (qualname, kind, range, marker). `None` for the module, which has no row in the outline: ask `kind_of` and
    /// `range_of`, which answer for both.
    pub(crate) fn def(&self, func: Func<'_>, style: MarkerStyle<'_>) -> Option<DefRef> {
        let index = func.outline_index()?;
        let defs = self
            .file_defs(func.file, style)
            .expect("a Func is made from an outline, so its file has one");
        Some(DefRef { defs, index })
    }

    /// What the def is: its kind in the outline, or `Module` for the file's own code.
    pub(crate) fn kind_of(&self, func: Func<'_>, style: MarkerStyle<'_>) -> DefKind {
        self.def(func, style)
            .map_or(DefKind::Module, |def| def.kind.clone())
    }

    /// Where the def is in its file, in bytes: its range in the outline, or the whole file for the module, so that a site's offset in the body is its
    /// position in the file.
    pub(crate) fn range_of(&self, func: Func<'_>, style: MarkerStyle<'_>) -> Span {
        match self.def(func, style) {
            Some(def) => def.range,
            None => {
                let end = func.file.node.text().len();
                Span::new(0, u32::try_from(end).unwrap_or(u32::MAX))
            }
        }
    }

    /// The node the def's body is read from: the def's node in the program's tree (`None` when the two parses disagreed, `FileDefs::nodes`), or the
    /// source file itself for the module. The one place that knows the module has no node in `FileDefs`.
    pub(crate) fn node_of(&self, func: Func<'_>, style: MarkerStyle<'_>) -> Option<P<Node>> {
        match func.outline_index() {
            Some(index) => self.file_defs(func.file, style)?.nodes[index],
            None => Some(func.file.node.as_node()),
        }
    }

    /// The text of the file at `relative`, as the program holds it.
    pub(crate) fn text_of(&self, relative: &str) -> Option<String> {
        Some(self.file(relative)?.node.text().to_owned())
    }

    /// The file's path relative to the root, with `/`; `None` for a file outside the root, such as the bundled libraries.
    pub(crate) fn relative_path(&self, file: File<'_>) -> Option<String> {
        under_root(&to_engine_path(&self.root), file.node.file_name()).map(str::to_owned)
    }

    /// The name of a file for an edge or a def: relative to the root with `/`, else the path from the root to it with `..` (`../shared/lib.ts`). Never
    /// the machine's absolute path, so the same project in another folder gives the same names (rule 9); the one exception is a file on another
    /// Windows drive, which has no path from the root (`place_between`).
    pub(crate) fn place_of(&self, file: File<'_>) -> String {
        self.relative_path(file)
            .unwrap_or_else(|| place_between(&to_engine_path(&self.root), file.node.file_name()))
    }

    /// Source code of any project: a TypeScript or JavaScript file (`Dialect::from_path`: an imported `.json` is in the program but is not code), not in
    /// `node_modules`, not a declaration file (`.d.ts`, `.d.mts`, `.d.cts`), not a bundled library. Where it lives is not asked: see `is_project_code`.
    fn is_source_code(&self, file: File<'_>) -> bool {
        let name = file.node.file_name();
        Dialect::from_path(name).is_some()
            && !self
                .program
                .is_source_file_default_library(file.node.path())
            && !tspath::is_declaration_file_name(name)
            && package_in_node_modules(name).is_none()
            && !self.program.is_source_file_from_external_library(file.node)
    }

    /// Code of the project itself: source code (`is_source_code`) **under the root** (D33). A file the program pulled in from outside the root is
    /// not walked: a call into it is an `External` named from the root (`place_of`), and `is_outside_root_source` says it is one to count.
    pub(crate) fn is_project_code(&self, file: File<'_>) -> bool {
        self.is_source_code(file) && self.relative_path(file).is_some()
    }

    /// Source code the program holds that is outside the root: it has a body that is not walked (D33), which `outside_root` counts. A declaration file
    /// out there has none, so it is not this.
    pub(crate) fn is_outside_root_source(&self, file: File<'_>) -> bool {
        self.is_source_code(file) && self.relative_path(file).is_none()
    }

    /// The config whose program this is, relative to the root; `None` for an inferred project.
    pub(crate) fn config_file(&self) -> Option<&str> {
        self.config_file.as_deref()
    }

    /// The config files that fed this program (see the field). What `environment_inputs` adds to its fixed globs.
    pub(crate) fn config_files(&self) -> &[String] {
        &self.config_files
    }

    /// A declaration file of the repo itself: under the root, not in `node_modules`, not a bundled library. Not project code (nothing in it is
    /// analyzed), but editing one changes how calls resolve, so a cache must know about it.
    pub(crate) fn is_repo_declaration_file(&self, file: File<'_>) -> bool {
        tspath::is_declaration_file_name(file.node.file_name())
            && self.package_of(file).is_none()
            && self.relative_path(file).is_some()
    }

    /// The package a file belongs to, for an `External` edge: `"lodash"`, `"@scope/pkg"`, or `"typescript:lib"` for a bundled library. `None` for
    /// the project's own files, declaration files included. `@types/x` is named `x`, the package it describes.
    pub(crate) fn package_of(&self, file: File<'_>) -> Option<String> {
        if self
            .program
            .is_source_file_default_library(file.node.path())
        {
            return Some(BUNDLED_LIBS.into());
        }
        package_in_node_modules(file.node.file_name())
    }
}

/// The program's one checker, held. The checker is behind a mutex that one thread cannot lock twice, and a second `get_type_checker` while a handle is out
/// does not fail: it waits for ever (H1). `Sem::checker` is the only way here to take a handle, and it says so at the line that takes a second one.
pub(super) struct HeldChecker<'s> {
    handle: CheckerHandle,
    held: &'s Cell<bool>,
}

impl Deref for HeldChecker<'_> {
    type Target = Checker;
    fn deref(&self) -> &Checker {
        &self.handle
    }
}

impl DerefMut for HeldChecker<'_> {
    fn deref_mut(&mut self) -> &mut Checker {
        &mut self.handle
    }
}

impl Drop for HeldChecker<'_> {
    fn drop(&mut self) {
        self.held.set(false);
    }
}

impl Sem {
    /// The checker, for one question at a time: drop the result before asking for it again. A second handle while one is held is a `debug_assert!`
    /// failure, where it would otherwise wait for ever.
    pub(super) fn checker(&self) -> HeldChecker<'_> {
        debug_assert!(
            !self.checker_held.get(),
            "one checker handle at a time: a second would wait for the first for ever"
        );
        let handle = self.program.get_type_checker(&Context::default());
        self.checker_held.set(true);
        HeldChecker {
            handle,
            held: &self.checker_held,
        }
    }
}

impl Drop for Sem {
    /// The only `unsafe` in the workspace: tsrs's contract for freeing a program (`tsrs_lint` does the same in `ProgramOwner::drop`).
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        let _scope = self.region.enter();
        // SAFETY: `open` made this program and shared nothing of it with another (no `update_program`); `Sem` holds no checker handle and
        // hands none out, and nothing reads `self.program` after this. "Hands none out" is true only while `Sem` has no handle type: the
        // first one (`File<'p>`, TS3 step 4) must carry `PhantomData<&'p Sem>`, so the compiler checks that none outlives this drop.
        unsafe { tsrs_compiler::free_unshared_program(self.program) };
    }
}

/// What `package_of` says for a bundled `lib.*.d.ts` (`typescript:lib`: the engine's own copy of TypeScript's libraries).
const BUNDLED_LIBS: &str = "typescript:lib";

/// The package a path inside `node_modules` belongs to, from the **last** `node_modules/` in it (pnpm nests them:
/// `node_modules/.pnpm/a@1/node_modules/a/…` is `a`). `@scope/pkg` keeps its scope; `@types/x` is `x` and `@types/scope__x` is `@scope/x`, the
/// packages they describe.
fn package_in_node_modules(path: &str) -> Option<String> {
    let (_, inside) = path.rsplit_once("/node_modules/")?;
    let mut parts = inside.split('/');
    let first = parts.next().filter(|part| !part.is_empty())?;
    if !first.starts_with('@') {
        return Some(first.to_owned());
    }
    let name = parts.next().filter(|part| !part.is_empty())?;
    if first == "@types" {
        return Some(match name.split_once("__") {
            Some((scope, name)) => format!("@{scope}/{name}"),
            None => name.to_owned(),
        });
    }
    Some(format!("{first}/{name}"))
}

/// Which defs own their marker (`FileDefs::owns_marker`): the first in source order (by where the def starts, not by the order of the outline) of each text.
fn marker_owners(outline: &Outline) -> Vec<bool> {
    let mut order: Vec<usize> = (0..outline.defs.len()).collect();
    order.sort_by_key(|&index| (outline.defs[index].range.start, index));
    let mut seen: HashSet<&str> = HashSet::new();
    let mut owns = vec![false; outline.defs.len()];
    for index in order {
        if let Some(marker) = outline.defs[index].marker.as_deref() {
            owns[index] = seen.insert(marker);
        }
    }
    owns
}

/// The node's own bytes: from its first token to its end. tsrs's `pos` includes the trivia before the node, so the start is moved past it. This is how
/// `typescript-syntax` measures a node too (`Parsed::span`), which is what lets a span from there find a node here.
fn own_span(text: &str, node: P<Node>) -> Span {
    let end = node.end().source_offset().unwrap_or(0);
    let pos = node.pos().source_offset().unwrap_or(0);
    Span::new(tsrs_scanner::skip_trivia(text, pos).min(end), end)
}
