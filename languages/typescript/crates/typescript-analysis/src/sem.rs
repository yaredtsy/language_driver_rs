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
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tsrs_ast::{self as ast, Kind, Node, NodeFlags, SourceFile};
use tsrs_compiler::{Program, ProgramOptions, ProgramRegions};
use tsrs_core::arena::Region;
use tsrs_core::{
    CompilerOptions, JsxEmit, ModuleKind, ModuleResolutionKind, P, ScriptTarget, Tristate, tspath,
};
use tsrs_tsoptions::{ParseConfigHost, ParsedCommandLine};
use tsrs_vfs::{FS, bundled, cachedvfs, osvfs};

use typescript_syntax::{Def, Dialect, MarkerStyle, Outline, Placement, Span};

use crate::OpenError;

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

/// A def of one file's outline (`typescript_syntax::outline`, the rule TS1 froze): a `File` and the index of the def in that file's outline. It holds no
/// tsrs pointer of its own, only the `File`, so it is valid for the `Sem`'s life and no longer, stays on one thread, and is not shared, exactly as `File`
/// is. `proof` pins all three for `Func` too.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Func<'p> {
    file: File<'p>,
    def: usize,
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
    /// One outline per (file, marker label), made on first use. A `Sem` is not `Sync`, so a `RefCell` is enough.
    file_defs: RefCell<FileDefsCache>,
    /// What the walk of each def's body found, made on first use, by the def's node: its sites, the defs it stops at, the references by name.
    bodies: RefCell<HashMap<P<Node>, Arc<Body>>>,
    // Fields drop in this order, after `Drop::drop` has freed the program: the loaders' regions, then the program's own.
    _loaders: Arc<ProgramRegions>,
    _region: Region,
}

impl Sem {
    /// Loads the program of the `tsconfig.json` (or `jsconfig.json`) nearest above `file`, looking no further up than `root`; with none, an
    /// inferred project built from `file` alone.
    /// `file` is relative to `root`, or absolute. A program that loads with type errors is not an error.
    pub fn open(root: &Path, file: &Path) -> Result<Sem, OpenError> {
        let root = root.canonicalize().map_err(|_| OpenError::NoSuchFile)?;
        let file = root
            .join(file)
            .canonicalize()
            .map_err(|_| OpenError::NoSuchFile)?;
        if !file.is_file() {
            return Err(OpenError::NoSuchFile);
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
            file_defs: RefCell::new(HashMap::new()),
            bodies: RefCell::new(HashMap::new()),
            _loaders: loaders,
            _region: region,
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

    /// Every function, method and class of the project's own files that can be walked (it has a node in the program's tree), in the program's file order
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
                ) && defs.nodes[index].is_some()
                {
                    found.push(Func { file, def: index });
                }
            }
        }
        found
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
        let _scope = self._region.enter();
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
        let made = Arc::new(FileDefs {
            outline,
            nodes,
            def_of,
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
        let defs = self.file_defs(self.file(file)?, style)?;
        Some(
            defs.outline
                .defs
                .iter()
                .filter(|def| def.qualname == qualname)
                .enumerate()
                .map(|(occurrence, def)| {
                    let mut found = crate::DefMatch {
                        occurrence: occurrence as u16,
                        kind: def.kind.clone(),
                        range: def.range,
                        marker: None,
                    };
                    found.marker = def.marker.clone();
                    found
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
        nodes[func.def] = None;
        let def_of = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| Some(((*node)?, index)))
            .collect();
        let replaced = Arc::new(FileDefs {
            outline: defs.outline.clone(),
            nodes,
            def_of,
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
        let defs = self.file_defs(file, style)?;
        let (def, _) = defs
            .outline
            .defs
            .iter()
            .enumerate()
            .filter(|(_, def)| def.qualname == qualname)
            .nth(usize::from(occurrence))?;
        Some(Func { file, def })
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
        let node = defs.nodes[func.def]?;
        if let Some(found) = self.bodies.borrow().get(&node) {
            return Some(Arc::clone(found));
        }
        let _scope = self._region.enter();
        let mut collector = Collector {
            text: func.file.node.text(),
            def_of: &defs.def_of,
            own: node,
            sites: Vec::new(),
            nested: Vec::new(),
            lits: Vec::new(),
            elems: Vec::new(),
            binds: Vec::new(),
            call_apply: Vec::new(),
            refs: Vec::new(),
            accesses: Vec::new(),
            destructured: Vec::new(),
            assigns: Vec::new(),
        };
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
        // Overloads are one def each (two signatures and the implementation): the one that runs is the one with a body, and a signature has nothing to
        // walk, so choosing the first would lose every call of the constructor.
        let mut candidates = defs
            .outline
            .defs
            .iter()
            .enumerate()
            .filter(|(_, def)| def.parent == Some(class.def) && def.name == "constructor")
            .map(|(index, _)| index);
        let first = candidates.next()?;
        let index = std::iter::once(first)
            .chain(candidates)
            .find(|&index| defs.nodes[index].is_some_and(|node| node.body().is_some()))
            .unwrap_or(first);
        Some(Func {
            file: class.file,
            def: index,
        })
    }

    /// What the outline says the def is (qualname, kind, range, marker).
    pub(crate) fn def(&self, func: Func<'_>, style: MarkerStyle<'_>) -> DefRef {
        let defs = self
            .file_defs(func.file, style)
            .expect("a Func is made from an outline, so its file has one");
        DefRef {
            defs,
            index: func.def,
        }
    }

    /// The text of the file at `relative`, as the program holds it.
    pub(crate) fn text_of(&self, relative: &str) -> Option<String> {
        Some(self.file(relative)?.node.text().to_owned())
    }

    /// The file's path relative to the root, with `/`; `None` for a file outside the root, such as the bundled libraries.
    pub(crate) fn relative_path(&self, file: File<'_>) -> Option<String> {
        let root = to_engine_path(&self.root);
        let relative = file.node.file_name().strip_prefix(root.as_str())?;
        Some(relative.strip_prefix('/')?.to_owned())
    }

    /// Code of the project itself: a TypeScript or JavaScript file (`Dialect::from_path`: an imported `.json` is in the program but is not code),
    /// not in `node_modules`, not a declaration file (`.d.ts`, `.d.mts`, `.d.cts`), not a bundled library.
    pub(crate) fn is_project_code(&self, file: File<'_>) -> bool {
        let name = file.node.file_name();
        Dialect::from_path(name).is_some()
            && !self
                .program
                .is_source_file_default_library(file.node.path())
            && !tspath::is_declaration_file_name(name)
            && package_in_node_modules(name).is_none()
            && !self.program.is_source_file_from_external_library(file.node)
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

impl Drop for Sem {
    /// The only `unsafe` in the workspace: tsrs's contract for freeing a program (`tsrs_lint` does the same in `ProgramOwner::drop`).
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        let _scope = self._region.enter();
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

/// The node's own bytes: from its first token to its end. tsrs's `pos` includes the trivia before the node, so the start is moved past it. This is how
/// `typescript-syntax` measures a node too (`Parsed::span`), which is what lets a span from there find a node here.
fn own_span(text: &str, node: P<Node>) -> Span {
    let end = node.end().source_offset().unwrap_or(0);
    let pos = node.pos().source_offset().unwrap_or(0);
    Span::new(tsrs_scanner::skip_trivia(text, pos).min(end), end)
}

/// The plain calls in one def's body, as the callee expression's span of each. It follows `tsrs_ls`'s `CallSiteCollector` (`callhierarchy.rs`, `collect`),
/// the same node kinds and the same places where it does not descend, with the plan's differences: it stops at any nested node that is a def of the
/// **outline** (`def_of`), not at what tsrs's call hierarchy calls a declaration, so an anonymous function is part of the def around it and a named one is
/// not; and a class's static block and property initializers are the class's own body. Only calls and constructions are sites so far (`f(x)`, `a.b(x)`,
/// `a?.b(x)`, `super(x)`, `new C(x)`), a tagged template's tag, a decorator, a JSX element that is not intrinsic, and a property access that runs an accessor.
struct Collector<'a> {
    text: &'a str,
    def_of: &'a HashMap<P<Node>, usize>,
    /// The def being walked: it is in `def_of` too, and must not stop its own walk.
    own: P<Node>,
    sites: Vec<CallNode>,
    /// The defs the walk stopped at (they are in `def_of`), with whether each is a named function expression **handed over where it is written**
    /// (`xs.map(function step() {})`): the only nested def that is handed over by its own place. The others (a method or arrow of a literal, a `const` arrow,
    /// a function declaration) are handed over by the literal or the name that holds them.
    nested: Vec<(usize, bool)>,
    /// The object literals written in a position where a value is handed over (`literal_is_handed`): what a literal hands over is its function members.
    lits: Vec<P<Node>>,
    /// Element accesses (`a["k"]`) in a hand-over position, and the targets of `x.bind(…)` in one and of `x.call(…)` / `x.apply(…)` anywhere (`x`): what no edge
    /// covers, counted by `Sem::handed_over`.
    elems: Vec<P<Node>>,
    binds: Vec<P<Node>>,
    call_apply: Vec<P<Node>>,
    /// The names in a position where a value is **handed over** (`is_handed_over`): an identifier or a property access, or a shorthand property, with the
    /// span of the name. What each names is asked of the checker later (`Sem::handed_over`).
    refs: Vec<(Span, P<Node>)>,
    /// The property accesses that are not callees (`note_access`): which are accessors is the checker's to say.
    accesses: Vec<AccessCandidate>,
    /// The elements of an object destructuring of a variable (`const { a, b: { c } } = x`): each reads a property without a property access, so an accessor
    /// among them is run by no site (`Uncovered::DestructuredAccessor`, counted by `Sem::handed_over`).
    destructured: Vec<P<Node>>,
    /// The assignments whose left side is an object pattern (`({ load } = api)`): the pattern and the value it takes from (`note_assignment`).
    assigns: Vec<(P<Node>, P<Node>)>,
}

/// What reading one def's body found. `sites` is the calls; `nested` and `refs` are what the body hands over (`Sem::handed_over`).
struct Body {
    destructured: Vec<P<Node>>,
    assigns: Vec<(P<Node>, P<Node>)>,
    sites: Vec<CallNode>,
    nested: Vec<(usize, bool)>,
    lits: Vec<P<Node>>,
    elems: Vec<P<Node>>,
    binds: Vec<P<Node>>,
    call_apply: Vec<P<Node>>,
    refs: Vec<(Span, P<Node>)>,
}

/// A site found in a body: the span of the expression that is resolved (absolute, in the file), that expression, and the node the site is (a call, a `new`, a
/// tagged template, a JSX element, a decorator, a property access). Kept privately by `Sem`, per def, so that `callee` can ask the checker about a site given
/// only its index: the index is plain data, the node never leaves `sem.rs`.
#[derive(Clone, Copy)]
struct CallNode {
    span: Span,
    /// What is resolved: a call's callee, a `new`'s class, a template's tag, a JSX element's tag name, a decorator's expression, and for an accessor the access
    /// itself (`a.b`), or the key of `a["b"]`.
    expr: P<Node>,
    /// The node of the site, which the checker's resolved signature is asked about.
    call: P<Node>,
    kind: SiteKind,
}

/// What a site is. The first four are calls of a function the source names; an accessor is a call the source does not write: `a.b` runs a getter or a setter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SiteKind {
    /// `f(x)`, `a.b(x)`, `new C(x)`, `super(x)`
    Call,
    /// `<Tag />`: the tag name
    Jsx,
    /// `` tag`text` ``: the tag
    Tagged,
    /// `@decorator` or `@decorator(1)`
    Decorator,
    /// `a.b` (or `a["b"]`) where `b` is an accessor: the getter for a read, the setter for a write, both for `+=` and `++`.
    Accessor(Access),
}

/// How a property access uses the property.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Access {
    Read,
    Write,
    ReadWrite,
}

impl Access {
    fn reads(self) -> bool {
        self != Access::Write
    }
    fn writes(self) -> bool {
        self != Access::Read
    }
}

/// A property access that may be an accessor: asked of the checker once the body is read (`Sem::body`).
#[derive(Clone, Copy)]
struct AccessCandidate {
    node: P<Node>,
    /// Where the checker is asked for the property's symbol: the access, or the key of an element access.
    location: P<Node>,
    access: Access,
}

impl Collector<'_> {
    fn collect(&mut self, node: Option<P<Node>>) {
        let Some(node) = node else {
            return;
        };
        // ambient code is a declaration, not a body
        if node.flags().intersects(NodeFlags::Ambient) {
            return;
        }
        // a nested def's calls are its own. As in tsrs, a nested class still owns the calls in its computed member names.
        if node != self.own
            && let Some(&def) = self.def_of.get(&node)
        {
            self.nested.push((def, is_handed_function_expression(node)));
            if ast::is_class_like(node) {
                for &member in node.members() {
                    if let Some(name) = member.name()
                        && ast::is_computed_property_name(name)
                    {
                        self.collect(name.expression());
                    }
                }
            }
            return;
        }
        match node.kind() {
            // cannot contain a callable node; a name that is not being called may be a function handed over
            Kind::Identifier => {
                // `{ f }` names a value and is a declaration name at once: the shorthand is what the checker is asked about (its symbol for the
                // name is the property; `get_shorthand_assignment_value_symbol` is the variable)
                match node.parent() {
                    Some(parent)
                        if parent.kind() == Kind::ShorthandPropertyAssignment
                            && parent.name() == Some(node) =>
                    {
                        self.reference(node, parent)
                    }
                    _ => self.reference(node, node),
                }
                return;
            }
            Kind::ImportEqualsDeclaration
            | Kind::ImportDeclaration
            | Kind::ExportDeclaration
            | Kind::InterfaceDeclaration
            | Kind::TypeAliasDeclaration => return,
            // not the type side of an assertion
            Kind::TypeAssertionExpression | Kind::AsExpression | Kind::SatisfiesExpression => {
                self.collect(node.expression());
                return;
            }
            // not the type of a variable or parameter
            Kind::VariableDeclaration | Kind::Parameter => {
                if node.kind() == Kind::VariableDeclaration
                    && node.initializer().is_some()
                    && let Some(pattern) = node
                        .name()
                        .filter(|name| name.kind() == Kind::ObjectBindingPattern)
                {
                    self.note_destructured(pattern);
                }
                self.collect(node.name());
                self.collect(node.initializer());
                return;
            }
            Kind::CallExpression => {
                if is_plain_call(node)
                    && let Some(callee) = node.expression()
                {
                    self.sites.push(CallNode {
                        span: own_span(self.text, callee),
                        expr: callee,
                        call: node,
                        kind: SiteKind::Call,
                    });
                }
                self.note_bound_or_applied(node);
                // not the type arguments
                self.collect(node.expression());
                for &argument in node.arguments() {
                    self.collect(Some(argument));
                }
                return;
            }
            Kind::NewExpression => {
                // a construction is a site on the class expression, as a call is one on the callee
                if let Some(class) = node.expression() {
                    self.sites.push(CallNode {
                        span: own_span(self.text, class),
                        expr: class,
                        call: node,
                        kind: SiteKind::Call,
                    });
                }
                self.collect(node.expression());
                for &argument in node.arguments() {
                    self.collect(Some(argument));
                }
                return;
            }
            // `` tag`text` ``: the tag is called
            Kind::TaggedTemplateExpression => {
                let tag = node.as_tagged_template_expression().tag;
                self.sites.push(CallNode {
                    span: own_span(self.text, tag),
                    expr: tag,
                    call: node,
                    kind: SiteKind::Tagged,
                });
            }
            // `<Tag />`: a component is called, an intrinsic element (`<div>`, `<my-el>`, `<svg:rect>`) is not
            Kind::JsxOpeningElement | Kind::JsxSelfClosingElement => {
                let tag = node.tag_name();
                if !is_intrinsic_jsx_tag(tag) {
                    self.sites.push(CallNode {
                        span: own_span(self.text, tag),
                        expr: tag,
                        call: node,
                        kind: SiteKind::Jsx,
                    });
                }
            }
            // `@decorator`, `@decorator(1)`: the expression is called with what it decorates
            Kind::Decorator => {
                if let Some(expression) = node.expression() {
                    self.sites.push(CallNode {
                        span: own_span(self.text, expression),
                        expr: expression,
                        call: node,
                        kind: SiteKind::Decorator,
                    });
                }
            }
            // `({ load } = api)`: the names on the left take members of the value on the right, and no edge follows them
            Kind::BinaryExpression => self.note_assignment(node),
            // `a["k"]` handed over: no edge covers it (a counted remainder)
            Kind::ElementAccessExpression if is_handed_over(node) => {
                self.elems.push(node);
                self.note_access(node);
            }
            Kind::ElementAccessExpression => self.note_access(node),
            // an object literal handed over where it is written hands over its function members
            Kind::ObjectLiteralExpression if literal_is_handed(node) => self.lits.push(node),
            // `a.b`: the access is the name (its own name child is not looked at); the object is walked
            Kind::PropertyAccessExpression if !ast::is_part_of_type_node(node) => {
                self.reference(node, node);
                self.note_access(node);
                self.collect(node.expression());
                return;
            }
            _ => {}
        }
        if ast::is_part_of_type_node(node) {
            return;
        }
        node.for_each_child(&mut |child| {
            self.collect(Some(child));
            false
        });
    }

    /// The elements of an object binding pattern, and of the patterns nested in it. Which of them name one property (not a rest element, not a computed key) is
    /// `bound_symbol`'s to say.
    fn note_destructured(&mut self, pattern: P<Node>) {
        for &element in pattern.elements() {
            if element.kind() != Kind::BindingElement {
                continue;
            }
            self.destructured.push(element);
            if let Some(nested) = element
                .name()
                .filter(|name| name.kind() == Kind::ObjectBindingPattern)
            {
                self.note_destructured(nested);
            }
        }
    }

    /// `({ a, b: c } = value)`: an assignment whose left side is an object pattern. Remembered with its two sides, for `Sem::handed_over`.
    fn note_assignment(&mut self, node: P<Node>) {
        let binary = node.as_binary_expression();
        if binary.operator_token.kind() == Kind::EqualsToken
            && binary.left().kind() == Kind::ObjectLiteralExpression
        {
            self.assigns.push((binary.left(), binary.right()));
        }
    }

    /// `a.b` and `a["b"]` that are not callees and not deleted: the getter or setter runs, if `b` is an accessor. Remembered with how the property is used.
    fn note_access(&mut self, node: P<Node>) {
        if ast::is_part_of_type_node(node) || is_callee_position(node) {
            return;
        }
        let location = if node.kind() == Kind::ElementAccessExpression {
            let key = node.as_element_access_expression().argument_expression;
            if !ast::is_string_literal_like(key) {
                return;
            }
            key
        } else {
            node
        };
        let mut outer = node;
        while let Some(parent) = outer.parent()
            && matches!(
                parent.kind(),
                Kind::NonNullExpression
                    | Kind::AsExpression
                    | Kind::SatisfiesExpression
                    | Kind::TypeAssertionExpression
            )
        {
            outer = parent;
        }
        if outer
            .parent()
            .is_some_and(|parent| parent.kind() == Kind::DeleteExpression)
        {
            return;
        }
        let access = if !ast::is_write_access(outer) {
            Access::Read
        } else if ast::is_write_only_access(outer) {
            Access::Write
        } else {
            Access::ReadWrite
        };
        self.accesses.push(AccessCandidate {
            node,
            location,
            access,
        });
    }

    /// `x.bind(…)` in a hand-over position, and `x.call(…)` / `x.apply(…)` anywhere: `x` is a function that is handed over bound, or called through `call`, and
    /// no edge covers either. Remembered for the count.
    fn note_bound_or_applied(&mut self, call: P<Node>) {
        let Some(callee) = call.expression() else {
            return;
        };
        if callee.kind() != Kind::PropertyAccessExpression {
            return;
        }
        let (Some(target), Some(name)) = (callee.expression(), callee.name()) else {
            return;
        };
        match name.text() {
            "bind" if is_handed_over(call) => self.binds.push(target),
            "call" | "apply" => self.call_apply.push(target),
            _ => {}
        }
    }

    /// A name where a value is handed over: remembered with the span of the name, and the node to ask the checker about.
    fn reference(&mut self, name: P<Node>, asked: P<Node>) {
        if is_handed_over(name) {
            self.refs.push((own_span(self.text, name), asked));
        }
    }

    /// What a def's walk starts from: the parts of the node that are its body (`tsrs_ls`'s `collect_call_sites`, with the plan's class rule).
    fn body_of(&mut self, node: P<Node>) {
        match node.kind() {
            Kind::FunctionDeclaration
            | Kind::FunctionExpression
            | Kind::ArrowFunction
            | Kind::MethodDeclaration
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::Constructor => {
                for &parameter in node.parameters() {
                    self.collect(Some(parameter));
                }
                self.collect(node.body());
            }
            Kind::ClassDeclaration | Kind::ClassExpression => {
                if let Some(modifiers) = node.modifiers() {
                    for &modifier in modifiers.nodes() {
                        self.collect(Some(modifier));
                    }
                }
                if let Some(heritage) = ast::get_class_extends_heritage_element(node) {
                    self.collect(heritage.expression());
                }
                for &member in node.members() {
                    if ast::can_have_modifiers(member)
                        && let Some(modifiers) = member.modifiers()
                    {
                        for &modifier in modifiers.nodes() {
                            self.collect(Some(modifier));
                        }
                    }
                    // `constructor(@Inject(T) x)`, `method(@Body() b)`: the decorators of a parameter run when the class is defined, so they belong to the class
                    if matches!(
                        member.kind(),
                        Kind::Constructor
                            | Kind::MethodDeclaration
                            | Kind::GetAccessor
                            | Kind::SetAccessor
                    ) {
                        for &parameter in member.parameters() {
                            if let Some(modifiers) = parameter.modifiers() {
                                for &modifier in modifiers.nodes() {
                                    if modifier.kind() == Kind::Decorator {
                                        self.collect(Some(modifier));
                                    }
                                }
                            }
                        }
                    }
                    match member.kind() {
                        Kind::PropertyDeclaration => self.collect(member.initializer()),
                        Kind::ClassStaticBlockDeclaration => {
                            self.collect(Some(member.as_class_static_block_declaration().body));
                        }
                        _ => {}
                    }
                }
            }
            Kind::ModuleDeclaration => {
                if let Some(body) = node.body()
                    && ast::is_module_block(body)
                {
                    for &statement in body.statements() {
                        self.collect(Some(statement));
                    }
                }
            }
            // an interface, a method signature, an enum, a type alias: nothing to walk
            _ => {}
        }
    }
}

/// Whether `node` is what is called where it stands: the callee of a call or a `new`, a tagged template's tag, a decorator's expression, a JSX tag name (the
/// closing one too), however many parentheses and assertions are between. Such an access is resolved as the callee, and is not an accessor site of its own.
fn is_callee_position(node: P<Node>) -> bool {
    let mut at = node;
    while let Some(parent) = at.parent() {
        match parent.kind() {
            Kind::ParenthesizedExpression
            | Kind::AsExpression
            | Kind::SatisfiesExpression
            | Kind::TypeAssertionExpression
            | Kind::NonNullExpression => at = parent,
            Kind::CallExpression | Kind::NewExpression | Kind::Decorator => {
                return parent.expression() == Some(at);
            }
            Kind::TaggedTemplateExpression => {
                return parent.as_tagged_template_expression().tag == at;
            }
            Kind::JsxOpeningElement | Kind::JsxSelfClosingElement | Kind::JsxClosingElement => {
                return parent.tag_name() == at;
            }
            _ => return false,
        }
    }
    false
}

/// A JSX tag that names no value: `<div>`, `<my-el>` (a lower-case first letter, or a dash), `<svg:rect>` (a namespaced name). TypeScript's rule
/// (`isIntrinsicJsxName`); a tag that is an access (`<props.Icon />`) or starts with a capital is a component.
fn is_intrinsic_jsx_tag(tag: P<Node>) -> bool {
    match tag.kind() {
        Kind::JsxNamespacedName => true,
        Kind::Identifier => {
            let name = tag.text();
            name.starts_with(|c: char| c.is_lowercase()) || name.contains('-')
        }
        _ => false,
    }
}

/// The expression under `(x)`, `x as T`, `x satisfies T`, `<T>x` and `x!`, however many.
fn unwrap_expression(mut node: P<Node>) -> P<Node> {
    while matches!(
        node.kind(),
        Kind::ParenthesizedExpression
            | Kind::AsExpression
            | Kind::SatisfiesExpression
            | Kind::TypeAssertionExpression
            | Kind::NonNullExpression
    ) {
        match node.expression() {
            Some(inner) => node = inner,
            None => break,
        }
    }
    node
}

/// A def's node that is a **named function expression handed over where it is written**: `xs.map(function step() {})`, `register(function step() {})`.
/// Not `const f = function step() {}` or `{ cb: function step() {} }` (a binding or a property: the name or the literal that holds it hands it over).
fn is_handed_function_expression(node: P<Node>) -> bool {
    node.kind() == Kind::FunctionExpression && node.name().is_some() && handed_position(node, true)
}

/// Whether the value `name` stands for is **handed over** where it is written, and not called, read or tested: an argument of a call or of `new`, the
/// value of a property (`key: f`, and `{ f }`), an element of an array, an initializer or the right side of `=`, the value a function returns (`return
/// f`, an arrow's concise body), a JSX attribute's value or a JSX child. Through the wrappers that keep the value (`(f)`, `f as T`, `f satisfies T`,
/// `<T>f`, `f!`), the branches of a conditional, and the right side of `&&`, `||` and `??`.
///
/// Not a hand-over, though it names the value: a callee (`f()`, `(a ? f : g)()`), a tagged template's tag, a decorator, a JSX tag, a condition (`if (f)`,
/// `f ? a : b`), `typeof f`, a comparison, `!f`, the object of an access (`f.length`, `f[0]`), a template's substitution. Those are the language's other
/// sites or reads of the def, and drawing them as "passed" would say the def is given to something.
fn is_handed_over(name: P<Node>) -> bool {
    handed_position(name, false)
}

/// Whether an **object literal** is handed over where it is written: the same positions as a name (`is_handed_over`), except that a literal that is only
/// bound (the initializer of a variable, a property value, a default) is made, not handed over: it is handed over by the name that holds it.
fn literal_is_handed(literal: P<Node>) -> bool {
    handed_position(literal, true)
}

/// `is_handed_over` and `literal_is_handed`: where `value` is handed over. `creating`: the value is a literal or a function expression, which a binding
/// (an initializer, a property value, a default) only makes.
fn handed_position(value: P<Node>, creating: bool) -> bool {
    let mut at = value;
    loop {
        let Some(parent) = at.parent() else {
            return false;
        };
        match parent.kind() {
            Kind::ParenthesizedExpression
            | Kind::AsExpression
            | Kind::SatisfiesExpression
            | Kind::TypeAssertionExpression
            | Kind::NonNullExpression => at = parent,
            Kind::ConditionalExpression => {
                if parent.as_conditional_expression().condition == at {
                    return false;
                }
                at = parent;
            }
            Kind::BinaryExpression => {
                let binary = parent.as_binary_expression();
                if binary.right() != at {
                    return false;
                }
                match binary.operator_token.kind() {
                    Kind::EqualsToken => {
                        return !matches!(
                            binary.left().kind(),
                            Kind::ObjectLiteralExpression | Kind::ArrayLiteralExpression
                        );
                    }
                    Kind::AmpersandAmpersandToken
                    | Kind::BarBarToken
                    | Kind::QuestionQuestionToken => at = parent,
                    _ => return false,
                }
            }
            Kind::CallExpression | Kind::NewExpression => {
                return parent.expression() != Some(at);
            }
            // `const { load } = api` and `({ load } = api)` read the members they name: the names are followed (`bound_member`), the initializer hands nothing over
            Kind::VariableDeclaration | Kind::Parameter
                if parent.initializer() == Some(at)
                    && parent.name().is_some_and(ast::is_binding_pattern) =>
            {
                return false;
            }
            Kind::PropertyAssignment
            | Kind::VariableDeclaration
            | Kind::Parameter
            | Kind::BindingElement
            | Kind::PropertyDeclaration => return !creating && parent.initializer() == Some(at),
            Kind::ArrowFunction => return parent.body() == Some(at),
            Kind::ShorthandPropertyAssignment
            | Kind::ArrayLiteralExpression
            | Kind::ReturnStatement
            | Kind::JsxExpression => return true,
            _ => return false,
        }
    }
}

/// `f(x)`, `a.b(x)`, `a?.b(x)`, `super(x)`. Not `import("./m")` and not `require("./m")`: that is loading a module, not calling into a function.
fn is_plain_call(call: P<Node>) -> bool {
    let Some(callee) = call.expression() else {
        return false;
    };
    if callee.kind() == Kind::ImportKeyword {
        return false;
    }
    let is_require = callee.kind() == Kind::Identifier
        && callee.text() == "require"
        && matches!(call.arguments(), [argument] if ast::is_string_literal_like(*argument));
    !is_require
}

/// What `NotInProject` names when there was no config: the program was inferred from the file alone.
const INFERRED: &str = "(inferred project)";

/// Reads a `tsconfig.json` / `jsconfig.json`: its options, its files and its references.
fn load_config(path: &Path, host: &ConfigHost) -> Result<ParsedCommandLine, OpenError> {
    let (config, diagnostics) = tsrs_tsoptions::get_parsed_command_line_of_config_file(
        &to_engine_path(path),
        None,
        None,
        host,
        None,
    );
    if let Some(first) = diagnostics.first() {
        return Err(OpenError::ConfigError(first.localize()));
    }
    let config =
        config.ok_or_else(|| OpenError::ConfigError("the config file could not be read".into()))?;
    // A config with a syntax error still parses, its errors kept on its own source file (as `tsrs_lint`'s `find_tsconfig` reads them).
    if let Some(first) = config
        .config_file
        .and_then(|file| file.source_file.diagnostics().first().copied())
    {
        return Err(OpenError::ConfigError(first.localize()));
    }
    Ok(config)
}

/// The program of a folder with no config: `file` alone is the root, and its imports pull in the rest. The options are `tsrs_lint`'s
/// for files no config claims, except that nothing is emitted (`no_emit`) and a `.js` file is read, never checked (`check_js` off).
fn inferred_config(file: &Path, host: &ConfigHost) -> ParsedCommandLine {
    tsrs_tsoptions::new_parsed_command_line(
        P::new(CompilerOptions {
            allow_js: Tristate::True,
            check_js: Tristate::False,
            no_emit: Tristate::True,
            module: ModuleKind::ES_NEXT,
            module_resolution: ModuleResolutionKind::BUNDLER,
            target: ScriptTarget::ES2022,
            jsx: JsxEmit::REACT_JSX,
            allow_importing_ts_extensions: Tristate::True,
            es_module_interop: Tristate::True,
            resolve_json_module: Tristate::True,
            ..Default::default()
        }),
        vec![to_engine_path(file)],
        Vec::new(),
        tspath::ComparePathsOptions {
            current_directory: host.cwd.clone(),
            use_case_sensitive_file_names: host.fs.use_case_sensitive_file_names(),
        },
    )
}

/// The nearest `tsconfig.json`, else `jsconfig.json`, in `dir` or above it, up to `root`: the editor's upward search, bounded by the
/// repository.
fn find_config(root: &Path, dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .take_while(|dir| is_within(root, dir))
        .find_map(|dir| {
            ["tsconfig.json", "jsconfig.json"]
                .iter()
                .map(|name| dir.join(name))
                .find(|path| path.is_file())
        })
}

/// Whether `dir` is `root` or below it, compared as the engine spells paths. `root` is canonical (on Windows verbatim: `\\?\D:\repo`), while a directory
/// the climb of `find_owner` rebuilds from a config's engine path is not (`D:/repo/sub`): `Path::starts_with` would call them different places and the climb
/// would find nothing above the nearest config.
fn is_within(root: &Path, dir: &Path) -> bool {
    Path::new(&to_engine_path(dir)).starts_with(to_engine_path(root))
}

/// What the search for the config that owns a file found.
enum Owner {
    /// The config whose program holds the file, as engine path and parsed.
    Config(String, Box<ParsedCommandLine>),
    /// There are configs above the file but none of them, nor any they reference, holds it. The nearest one's engine path.
    NoneHolds(String),
    /// No config above the file, up to the root.
    NoConfig,
}

/// Which config owns `file`, as `tsrs_lint`'s `find_tsconfig` (and tsserver) decide, but bounded by `root`: the nearest config, then the configs it
/// references (a solution config has `"files": []` and only references: the Vite layout), then the config above that, and so on; the first whose
/// file list holds the file. A `composite` config must hold it by its own patterns. `disableSolutionSearching` stops the climb.
///
/// Every config it reads goes into `inputs` (see `Sem::config_files`).
///
/// A syntax error in the **nearest** config is the person's to fix (`ConfigError`). One in a config only reached by a reference or a climb is
/// skipped, as `tsrs_lint` does (it may have nothing to do with this file), and listed in `skipped` for `NotInProject` to carry.
fn find_owner(
    root: &Path,
    file: &Path,
    host: &ConfigHost,
    inputs: &mut BTreeSet<String>,
    skipped: &mut Vec<String>,
) -> Result<Owner, OpenError> {
    let Some(nearest) = file.parent().and_then(|dir| find_config(root, dir)) else {
        return Ok(Owner::NoConfig);
    };
    let nearest = to_engine_path(&nearest);
    let file_name = to_engine_path(file);
    let file_path = tspath::to_path(
        &file_name,
        &host.cwd,
        host.fs.use_case_sensitive_file_names(),
    );

    let mut visited = HashSet::new();
    let mut next = Some(nearest.clone());
    while let Some(config_name) = next {
        let mut queue = VecDeque::from([config_name.clone()]);
        let mut disable_solution_searching = false;
        while let Some(name) = queue.pop_front() {
            if !visited.insert(name.clone()) {
                continue;
            }
            // read, so a change to it can change the answer; a broken one too, because fixing it would
            inputs.insert(name.clone());
            let config = match load_config(Path::new(&name), host) {
                Ok(config) => config,
                Err(error) if name == nearest => return Err(error),
                Err(error) => {
                    // never silent: `NotInProject` lists these, as the likely cause of "not in project"
                    let reason = match error {
                        OpenError::ConfigError(reason) => reason,
                        other => other.to_string(),
                    };
                    skipped.push(format!("{}: {reason}", relative_to(root, &name)));
                    continue;
                }
            };
            inputs.extend(config.extended_source_files());
            if name == config_name {
                disable_solution_searching = config
                    .compiler_options()
                    .is_some_and(|options| options.disable_solution_searching.is_true());
            }
            let holds = !config.file_names().is_empty()
                && (config
                    .compiler_options()
                    .is_none_or(|options| !options.composite.is_true())
                    || config.possibly_matches_file_name(&file_name))
                && config.file_names_by_path().contains_key(&file_path);
            if holds {
                return Ok(Owner::Config(name, Box::new(config)));
            }
            queue.extend(config.resolved_project_reference_paths().iter().cloned());
        }
        if disable_solution_searching {
            break;
        }
        // the config above: look from the parent of this config's directory
        next = Path::new(&config_name)
            .parent()
            .and_then(Path::parent)
            .and_then(|dir| find_config(root, dir))
            .map(|path| to_engine_path(&path));
    }
    Ok(Owner::NoneHolds(nearest))
}

/// An engine path made relative to `root` (the config's name in `NotInProject`).
fn relative_to(root: &Path, engine_path: &str) -> String {
    let root = to_engine_path(root);
    engine_path
        .strip_prefix(root.as_str())
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(engine_path)
        .to_owned()
}

/// tsrs spells every path absolute, with `/` as the separator.
fn to_engine_path(path: &Path) -> String {
    tspath::normalize_slashes(&without_verbatim_prefix(&path.to_string_lossy()))
}

/// On Windows `canonicalize` answers with a verbatim path (`\\?\D:\repo`, `\\?\UNC\server\share`), which tsrs cannot read: with `/` it becomes
/// `//?/D:/repo`, a path on a server named `?`. This gives the ordinary spelling back (`D:\repo`, `\\server\share`). Any other path is unchanged.
fn without_verbatim_prefix(path: &str) -> String {
    match path.strip_prefix(r"\\?\") {
        Some(rest) => match rest.strip_prefix(r"UNC\") {
            Some(share) => format!(r"\\{share}"),
            None => rest.to_owned(),
        },
        None => path.to_owned(),
    }
}

/// What a site calls, and what a def is (TS4 step 3). The resolver is a port of `tsrs_ls`'s `resolve_call_hierarchy_declaration` (`callhierarchy.rs`, a
/// private module there, so it cannot be called: S-TS3 in `ENGINE.md`), which is what "follow the engine" (D7) means for a call: from the callee
/// expression, a declaration that is itself callable, else the declaration of the symbol the checker finds there, once.
mod resolve {
    use analyzer_core::graph::{DefInfo, StableDef};
    use analyzer_core::outline::DefKind;
    use tsrs_ast::{Symbol, SymbolFlags};
    use tsrs_checker::{Checker, Type, TypeFlags};
    use tsrs_compiler::Context;

    use super::*;

    /// What one declaration the engine found for a site is.
    #[derive(Clone, PartialEq, Eq)]
    pub(crate) enum Resolved<'p> {
        /// A def of project code: a function, method, accessor or constructor, or a class. The solver walks it.
        Def(Func<'p>),
        /// Outside project code: the package (`pkg`, `typescript:lib`, or the path of the repo's own `.d.ts`) and the declaration's own name.
        External { package: String, name: String },
        /// The engine gives no declaration to enter.
        Unknown(Why),
    }

    /// What is known about a site beyond what it resolves to, found while the declarations were at hand.
    #[derive(Clone, Copy, Default)]
    pub(crate) struct SiteFacts {
        /// The site is a JSX element: a class it constructs is a class component.
        pub(crate) jsx: bool,
        /// The callee is a getter (`b.fn()`): the edge is to the getter, and the call of what it returns has no edge.
        pub(crate) getter_result: bool,
    }

    /// What runs when a class is constructed: the first constructor up its chain of base classes, since a class with none uses its base's.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Init<'p> {
        /// The constructor to enter: the class's own, or the nearest base class's in project code.
        Enter(Func<'p>),
        /// No class on the chain declares one: there is nothing to enter.
        Nothing,
        /// The chain reaches a base class outside project code (a package's, a library's): its constructor is never entered, and the class is not
        /// the project's to look into.
        External,
        /// The chain reaches a base the checker cannot name as a class (`extends mixin(Base)`, a value of type `any`): we do not know.
        Unknown,
    }

    /// A def a walked body hands over: made where an expression is, or named where a value is handed over (`is_handed_over`).
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) struct Handed<'p> {
        pub(crate) func: Func<'p>,
        /// Written inside an expression (a method or arrow of a literal, a named function expression) and not named: it has no place in the body where
        /// its name is. Otherwise it was named (`onClick={handleClose}`, `xs.map(parse)`, `{ f }`) and `at` says where.
        pub(crate) written: bool,
        /// The span of the name, in the file (absolute); `None` for a def that is written inside an expression. One `Handed` for each place a def is
        /// named, so a def named twice is two, each at its own site (D22).
        pub(crate) at: Option<Span>,
    }

    /// What a body hands over or calls in a way **no edge covers**, counted per occurrence (D22 step 5): a function handed over by an element access with a
    /// literal key, a function handed over bound (`f.bind(…)`, `obj.m.bind(obj)`), a function called through `call` or `apply` (the tree has `Function.call`,
    /// not the function), a class named where a value is handed over (a class is built, so a passed edge cannot point at it).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Uncovered {
        ElementAccess,
        Bind,
        CallApply,
        Class,
        /// `const { value } = box` where `value` is a getter: the getter runs, and there is no property access to be its site.
        DestructuredAccessor,
        /// `const { a, ...others } = api; use(others)`: the rest object holds defs of `api`, handed over by the name, and a rest element is not followed.
        RestElement,
        /// `({ load } = api); use(load)`: the target takes a def of `api`, handed over by its name, and an assignment target is not followed.
        AssignmentDestructuring,
        /// One site of an instance field initializer of a class that a construction runs: the construction draws the constructor, not the initializers.
        ConstructInitializerSite,
        /// One site of a decorator of a class declared inside a walked body: the decorators run when the body does, and the class's own body is not a part of it.
        LocalClassDecoratorSite,
        /// `<Cls />` where `Cls` is a class with a `render` method of the project: the framework calls `render`, and nothing in the project does.
        ClassComponentRender,
        /// `b.fn()` where `fn` is a getter: the one edge is the getter, and the call of what it returns is a second call.
        GetterResultCall,
    }

    impl Uncovered {
        /// The key in `Stats.language`.
        pub(crate) fn counter(self) -> &'static str {
            match self {
                Uncovered::ElementAccess => "uncovered_element_access",
                Uncovered::Bind => "uncovered_bind",
                Uncovered::CallApply => "uncovered_call_apply",
                Uncovered::Class => "uncovered_class",
                Uncovered::DestructuredAccessor => "uncovered_destructured_accessor",
                Uncovered::RestElement => "uncovered_rest_element",
                Uncovered::AssignmentDestructuring => "uncovered_assignment_destructuring",
                Uncovered::ConstructInitializerSite => "uncovered_construct_initializer_sites",
                Uncovered::LocalClassDecoratorSite => "uncovered_local_class_decorator_sites",
                Uncovered::ClassComponentRender => "uncovered_class_component_render",
                Uncovered::GetterResultCall => "uncovered_getter_result_calls",
            }
        }
    }

    /// What reading one body found about hand-overs: the defs it hands over (each an edge), and what it hands over or calls that no edge covers.
    pub(crate) struct HandOvers<'p> {
        pub(crate) handed: Vec<Handed<'p>>,
        pub(crate) uncovered: Vec<Uncovered>,
    }

    /// Why a site has no target. Each is counted apart in `Stats.language` (D16, rule 8), so a missing edge can be explained from a number.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Why {
        /// A callback parameter, a prop, a store action, a field of type `() => void`: the engine answers with a function type, which has no body (D16).
        FunctionTypedValue,
        /// A value of type `any`, or an untyped JavaScript value.
        AnyOrUntyped,
        /// The receiver is a union (`JsonWriter | CsvWriter`) and a member of it has no declaration to enter (a member of type `any`, a function
        /// type). The members that do are targets; this marks the ones that are not, so the answer is not mistaken for complete.
        UnionReceiver,
        /// `obj[name]()` where the checker cannot narrow `name` to a literal.
        ComputedName,
        /// The engine found a declaration in project code that is not a def of the outline: a bug in one of the two rules (rule 8: counted).
        NotInOutline,
        /// `@decorator(1)`: the function the factory returned is called with what is decorated, and nothing names it (an arrow written in the factory, a
        /// function type): the second call of a decorator that takes arguments.
        DecoratorResult,
        /// None of the above: no symbol, or a symbol with nothing to call.
        NoDeclaration,
    }

    impl Why {
        /// The key in `Stats.language`.
        pub(crate) fn counter(self) -> &'static str {
            match self {
                Why::FunctionTypedValue => "function_typed_value",
                Why::AnyOrUntyped => "any_or_untyped",
                Why::UnionReceiver => "union_receiver",
                Why::ComputedName => "computed_name",
                Why::NotInOutline => "decl_not_in_outline",
                Why::DecoratorResult => "decorator_result",
                Why::NoDeclaration => "no_declaration",
            }
        }
    }

    impl Sem {
        /// What the site at `site_index` of `func` calls, as the engine's call hierarchy would say. Never empty: a site with no target is one
        /// `Unknown`. A union receiver, or merged declarations, give one `Resolved` per declaration, in (file, offset) order (rule 9).
        #[cfg(test)]
        pub(crate) fn callee(
            &self,
            func: Func<'_>,
            site_index: usize,
            style: MarkerStyle<'_>,
        ) -> Vec<Resolved<'_>> {
            self.callee_with_facts(func, site_index, style).0
        }

        /// `callee`, and what else is known about the site while the declarations are at hand (`SiteFacts`).
        pub(crate) fn callee_with_facts(
            &self,
            func: Func<'_>,
            site_index: usize,
            style: MarkerStyle<'_>,
        ) -> (Vec<Resolved<'_>>, SiteFacts) {
            let mut facts = SiteFacts::default();
            let Some(calls) = self.body(func, style) else {
                return (vec![Resolved::Unknown(Why::NoDeclaration)], facts);
            };
            let Some(call) = calls.sites.get(site_index) else {
                debug_assert!(false, "a site index outside the def's sites");
                return (vec![Resolved::Unknown(Why::NoDeclaration)], facts);
            };
            facts.jsx = call.kind == SiteKind::Jsx;
            let _scope = self._region.enter();
            // one checker, so one handle at a time: it is dropped before this returns
            let mut checker = self.program.get_type_checker(&Context::default());
            let callee = call.expr;
            if let SiteKind::Accessor(access) = call.kind {
                return (
                    self.accessor_targets(&mut checker, callee, access, style),
                    facts,
                );
            }
            if call.kind == SiteKind::Decorator
                && unwrap_expression(callee).kind() == Kind::CallExpression
            {
                return (
                    self.decorator_result_targets(&mut checker, call.call, style),
                    facts,
                );
            }
            let mut declarations =
                self.prefer_project_bodies(declarations_of(&mut checker, callee));
            let mut unresolved_members = false;
            if declarations.is_empty()
                && let Some((members, all_resolved)) =
                    declarations_of_union_symbol(&mut checker, callee)
            {
                // checked before the signature: for a union the checker's signature names the first member only, which would pass for the whole answer
                declarations = members;
                unresolved_members = !all_resolved;
            }
            if declarations.is_empty()
                && is_named_callee(callee)
                && !is_union_callee(&mut checker, callee)
            {
                // Only for a name, and not for a union of callables: the signature names one declaration, which is the answer when the callee is the
                // named thing and would pass for the whole answer when it is `(flag ? f : g)` (the checker reduces two alike function types to one)
                declarations = declarations_of_signature(&mut checker, call.call);
            }
            if declarations.is_empty() {
                if let Some(external) = self.external_declarations(&mut checker, callee) {
                    return (external, facts);
                }
                return (
                    vec![Resolved::Unknown(why_unknown(&mut checker, callee))],
                    facts,
                );
            }
            // the engine names the getter itself for `b.fn()`: one edge, and what the getter returns is called by a second call that has none
            facts.getter_result = declarations
                .iter()
                .any(|declaration| declaration.kind() == Kind::GetAccessor);
            let mut found: Vec<Resolved<'_>> = Vec::new();
            for declaration in declarations {
                let resolved = self.classify(&mut checker, declaration, style);
                if !found.contains(&resolved) {
                    found.push(resolved);
                }
            }
            if unresolved_members {
                // a member of the union that has no declaration to enter is not dropped
                found.push(Resolved::Unknown(Why::UnionReceiver));
            }
            (found, facts)
        }

        /// The property accesses of a body that run an accessor, as sites: for each candidate the checker's symbol of the property is asked, and it is a site when
        /// a declaration of it is an accessor of the half the access needs (a getter for a read, a setter for a write, both for `+=`). One checker for the whole
        /// body, and none when the body has no candidate. A property of a union that is an accessor in some members and a plain property in others is a site for
        /// the accessors only: the plain property runs nothing.
        pub(super) fn accessor_sites(
            &self,
            candidates: &[AccessCandidate],
            text: &str,
        ) -> Vec<CallNode> {
            if candidates.is_empty() {
                return Vec::new();
            }
            let mut checker = self.program.get_type_checker(&Context::default());
            candidates
                .iter()
                .filter(|candidate| {
                    accessor_declarations(&mut checker, candidate.location, candidate.access)
                        .is_some_and(|found| !found.is_empty())
                })
                .map(|candidate| CallNode {
                    span: own_span(text, candidate.node),
                    expr: candidate.location,
                    call: candidate.node,
                    kind: SiteKind::Accessor(candidate.access),
                })
                .collect()
        }

        /// What an accessor site runs: the getter for a read, the setter for a write, both for `+=` and `++`, as defs of the project or externals, in (file, offset)
        /// order. The site exists only when there is one (`accessor_sites`).
        fn accessor_targets(
            &self,
            checker: &mut Checker,
            location: P<Node>,
            access: Access,
            style: MarkerStyle<'_>,
        ) -> Vec<Resolved<'_>> {
            let mut declarations =
                accessor_declarations(checker, location, access).unwrap_or_default();
            declarations.sort_by_key(|declaration| {
                (
                    ast::get_source_file_of_node(*declaration)
                        .map(|file| file.file_name().to_owned()),
                    declaration.pos(),
                )
            });
            let mut found: Vec<Resolved<'_>> = Vec::new();
            for declaration in declarations {
                let resolved = self.classify(checker, declaration, style);
                if !found.contains(&resolved) {
                    found.push(resolved);
                }
            }
            if found.is_empty() {
                found.push(Resolved::Unknown(Why::NoDeclaration));
            }
            found
        }

        /// The second call of `@decorator(1)`: the function that `decorator(1)` returns is called with what is decorated. The first call is a site of its own (a
        /// plain call of `decorator`). The checker's resolved signature of the decorator names the returned function when it is a declaration that can be
        /// entered (`return apply;` with a function `apply`); an arrow written in the factory's `return` is part of the factory's body already (it is no def), and a
        /// function type (`ClassDecorator`) has nothing to enter, so those are `Unknown(DecoratorResult)`: counted, never dropped.
        fn decorator_result_targets(
            &self,
            checker: &mut Checker,
            decorator: P<Node>,
            style: MarkerStyle<'_>,
        ) -> Vec<Resolved<'_>> {
            let declarations = declarations_of_signature(checker, decorator);
            let mut found: Vec<Resolved<'_>> = Vec::new();
            for declaration in declarations {
                let resolved = self.classify(checker, declaration, style);
                if !found.contains(&resolved) {
                    found.push(resolved);
                }
            }
            if found.is_empty() {
                found.push(Resolved::Unknown(Why::DecoratorResult));
            }
            found
        }

        /// Our choice among the engine's declarations, for one case (pinned in `ENGINE.md` and the policy table): in a script file a function named like a
        /// library global (`fetch`, `open`, `close`, `name`) is the same symbol as the library's, so the checker lists both and a call to the project's
        /// function would also be an `External` edge to something never called. When a declaration with a **body** is in project code and every other one
        /// is in a declaration file (a bundled library, a package's `.d.ts`), only the project's stay. If no project declaration has a body, or another
        /// declaration is project code, nothing changes: the engine's list is the answer.
        fn prefer_project_bodies(&self, declarations: Vec<P<Node>>) -> Vec<P<Node>> {
            let is_declaration_file = |declaration: &P<Node>| {
                ast::get_source_file_of_node(*declaration)
                    .is_some_and(|file| tspath::is_declaration_file_name(file.file_name()))
            };
            let is_project_body = |declaration: &P<Node>| {
                declaration.body().is_some()
                    && !is_declaration_file(declaration)
                    && ast::get_source_file_of_node(*declaration).is_some_and(|file| {
                        self.is_project_code(File {
                            node: file,
                            _sem: PhantomData,
                            _one_thread: PhantomData,
                        })
                    })
            };
            let wins: Vec<P<Node>> = declarations
                .iter()
                .copied()
                .filter(|declaration| is_project_body(declaration))
                .collect();
            let others_are_libraries = declarations
                .iter()
                .filter(|declaration| !wins.contains(declaration))
                .all(is_declaration_file);
            if !wins.is_empty() && others_are_libraries {
                wins
            } else {
                declarations
            }
        }

        /// A callee whose every declaration is **outside** project code, when the engine's hierarchy has no body to enter: a named call into that package
        /// or library, an `External`. It is a holder of a value in a package (`export declare const useThing: () => number`, a property of function type:
        /// not one of D16's function-typed values, which are the ones declared in the project), and it is a library global declared as an interface and a
        /// `var`, which has no body either: `new Map()`, `String(x)`, `Date()`. `None` when any declaration is the project's, so a project's own holder
        /// stays D16's.
        fn external_declarations(
            &self,
            checker: &mut Checker,
            callee: P<Node>,
        ) -> Option<Vec<Resolved<'_>>> {
            // `super(…)` names the base class's constructor: the checker's symbol for the keyword is the constructor's *type* (`ErrorConstructor`), and
            // the same class must not have two names (`new Error()` is `Error`), so the base is read from the `extends` expression, as `init_chain` does
            let callee = if callee.kind() == Kind::SuperKeyword {
                let class = ast::find_ancestor(Some(callee), ast::is_class_like)?;
                ast::get_class_extends_heritage_element(class)?.expression()?
            } else {
                callee
            };
            let mut symbol = checker.get_symbol_at_location_exported(callee)?;
            if symbol.flags().intersects(SymbolFlags::Alias) {
                symbol = checker.get_aliased_symbol(symbol);
            }
            let declarations = symbol.declarations().to_vec();
            if declarations.is_empty() {
                return None;
            }
            let mut found: Vec<Resolved<'_>> = Vec::new();
            for declaration in declarations {
                let file = File {
                    node: ast::get_source_file_of_node(declaration)?,
                    _sem: PhantomData,
                    _one_thread: PhantomData,
                };
                if self.is_project_code(file) {
                    return None;
                }
                let mut resolved = self.classify(checker, declaration, MarkerStyle::docstring(""));
                // a property of an anonymous type (`declare const toolbox: { run: () => void }`) has no class or interface around it to qualify
                // its name: the receiver does (`toolbox.run`)
                if let Resolved::External { name, .. } = &mut resolved
                    && ast::is_property_access_expression(callee)
                    && !name.contains('.')
                    && let Some(receiver) = callee.expression()
                    && let Some(receiver_symbol) = checker.get_symbol_at_location_exported(receiver)
                {
                    *name = format!(
                        "{}.{name}",
                        checker.symbol_to_string_exported(receiver_symbol)
                    );
                }
                if !found.contains(&resolved) {
                    found.push(resolved);
                }
            }
            Some(found)
        }

        /// A declaration the engine found, as a def of project code, an external, or a bug to count.
        fn classify(
            &self,
            checker: &mut Checker,
            declaration: P<Node>,
            style: MarkerStyle<'_>,
        ) -> Resolved<'_> {
            let Some(source_file) = ast::get_source_file_of_node(declaration) else {
                return Resolved::Unknown(Why::NoDeclaration);
            };
            let file = File {
                node: source_file,
                _sem: PhantomData,
                _one_thread: PhantomData,
            };
            if !self.is_project_code(file) {
                let package = self.package_of(file).unwrap_or_else(|| {
                    // not in `node_modules` and not a bundled library: the repo's own `.d.ts`, named by where it is
                    self.relative_path(file)
                        .unwrap_or_else(|| source_file.file_name().to_owned())
                });
                // an arrow held by a property of an object literal is named by the property
                let named = if is_property_function_value(declaration) {
                    declaration.parent()
                } else {
                    Some(declaration)
                };
                let own = named
                    .and_then(|node| node.name())
                    .map(|name| name.text().to_owned())
                    .or_else(|| {
                        symbol_of_declaration(checker, declaration)
                            .map(|symbol| checker.symbol_to_string_exported(symbol))
                    })
                    .unwrap_or_default();
                let name = qualified_name(declaration, own);
                return Resolved::External { package, name };
            }
            let Some(defs) = self.file_defs(file, style) else {
                return Resolved::Unknown(Why::NotInOutline);
            };
            // The engine names the first signature of an overloaded function (its call hierarchy finds the implementation only when it is the symbol's
            // value declaration), and the outline's def is the implementation: so a function or method with no body is looked up among the
            // declarations of its own symbol for the one that has.
            let node = if defs.def_of.contains_key(&declaration) || declaration.body().is_some() {
                declaration
            } else {
                implementation_of(checker, declaration).unwrap_or(declaration)
            };
            match defs.def_of.get(&node).copied() {
                Some(def) => Resolved::Def(Func { file, def }),
                // A property that holds an arrow and is not a def by D21's rule (a `let`, a default export, an argument's literal): a holder of a
                // function, declared in the project, as a callback property is. The outline says it is not a def on purpose, so this is not the bug below.
                None if is_property_function_value(node) => {
                    Resolved::Unknown(Why::FunctionTypedValue)
                }
                None => {
                    // a bug in one of the two rules (what the engine calls callable, what the outline calls a def): a test must find it,
                    // and a release counts it
                    debug_assert!(
                        false,
                        "the engine's declaration in {} is not a def of the outline",
                        source_file.file_name()
                    );
                    Resolved::Unknown(Why::NotInOutline)
                }
            }
        }

        /// What constructing `class` runs: its own constructor, or the one it inherits. `Nothing` is an answer (no class up the chain declares one);
        /// `External` and `Unknown` are limits, and the caller counts them (rule 8) so that "no constructor to enter" is never read as "no constructor".
        /// With it, the classes the search went through: `class`, then each base class whose constructor it looked for, in order. A construction runs the
        /// instance field initializers of each of them (a class with no constructor of its own runs its base class's, and the base's initializers with it), and
        /// the last one is the class whose constructor is entered, if any.
        pub(crate) fn init_chain<'p>(
            &'p self,
            class: Func<'p>,
            style: MarkerStyle<'_>,
        ) -> (Init<'p>, Vec<Func<'p>>) {
            let mut seen = vec![class];
            let init = self.init_walk(class, style, &mut seen);
            (init, seen)
        }

        fn init_walk<'p>(
            &'p self,
            class: Func<'p>,
            style: MarkerStyle<'_>,
            seen: &mut Vec<Func<'p>>,
        ) -> Init<'p> {
            let mut class = class;
            loop {
                if let Some(constructor) = self.constructor_of(class, style) {
                    return Init::Enter(constructor);
                }
                let Some(defs) = self.file_defs(class.file, style) else {
                    return Init::Unknown;
                };
                let Some(node) = defs.nodes[class.def] else {
                    return Init::Unknown;
                };
                let Some(heritage) = ast::get_class_extends_heritage_element(node) else {
                    return Init::Nothing;
                };
                let Some(base) = heritage.expression() else {
                    return Init::Unknown;
                };
                let _scope = self._region.enter();
                let mut checker = self.program.get_type_checker(&Context::default());
                let mut resolved: Vec<Resolved<'_>> = Vec::new();
                let mut declarations = declarations_of(&mut checker, base);
                if declarations.is_empty() {
                    // a library's class (`Error`) is an interface and a `var`: the same fallback as a `new` of it
                    if let Some(external) = self.external_declarations(&mut checker, base) {
                        resolved = external;
                    }
                } else {
                    declarations.dedup();
                    for declaration in declarations {
                        let found = self.classify(&mut checker, declaration, style);
                        if !resolved.contains(&found) {
                            resolved.push(found);
                        }
                    }
                }
                match resolved.as_slice() {
                    [Resolved::Def(base_class)]
                        if self.def(*base_class, style).kind == DefKind::Class =>
                    {
                        // a class that extends itself is an error in the program, not a loop here
                        if seen.contains(base_class) {
                            return Init::Unknown;
                        }
                        seen.push(*base_class);
                        class = *base_class;
                    }
                    [_, ..]
                        if resolved
                            .iter()
                            .all(|r| matches!(r, Resolved::External { .. })) =>
                    {
                        return Init::External;
                    }
                    _ => return Init::Unknown,
                }
            }
        }

        /// The functions `func`'s body hands over instead of calling: the defs it writes inside an expression (no place for a name: `at` is `None`), and the
        /// project functions it names where a value is handed over (`is_handed_over`: an argument, a property value, an element, an initializer, a return, a
        /// JSX value, a shorthand property), each with the span of the name. Only functions with a body. Whether anything calls them is the caller's
        /// question (the graph's edges). Not looked at: an element access (`api["load"]`), `f.bind(…)` and `f.call(…)`, and a class named as a value.
        pub(crate) fn handed_over<'p>(
            &'p self,
            func: Func<'p>,
            style: MarkerStyle<'_>,
        ) -> HandOvers<'p> {
            let mut uncovered: Vec<Uncovered> = Vec::new();
            let Some(body) = self.body(func, style) else {
                return HandOvers {
                    handed: Vec::new(),
                    uncovered,
                };
            };
            let mut found: Vec<Handed<'p>> = Vec::new();
            // a named function expression handed over where it is written (`xs.map(function step() {})`): at its name
            if let Some(defs) = self.file_defs(func.file, style) {
                for &(def, handed) in &body.nested {
                    let held = Func {
                        file: func.file,
                        def,
                    };
                    if handed && self.is_function_with_body(held, style) {
                        found.push(Handed {
                            func: held,
                            written: true,
                            at: Some(defs.outline.defs[def].name_range),
                        });
                    }
                }
                // an object literal handed over where it is written hands over its function members, each at its own key
                for &literal in &body.lits {
                    let mut carried = Vec::new();
                    self.functions_of_literal(func.file, &defs, literal, &mut carried);
                    for (held, key) in carried {
                        let handed = Handed {
                            func: held,
                            written: true,
                            at: Some(key),
                        };
                        if !found.contains(&handed) {
                            found.push(handed);
                        }
                    }
                }
            }
            // a class declared in this body: its decorators run when the body does, and the class's own body is not a part of it
            if let Some(defs) = self.file_defs(func.file, style) {
                for &(def, _) in &body.nested {
                    if defs.outline.defs[def].kind == DefKind::Class {
                        let class = Func {
                            file: func.file,
                            def,
                        };
                        uncovered.extend(std::iter::repeat_n(
                            Uncovered::LocalClassDecoratorSite,
                            self.decorator_sites(class, style),
                        ));
                    }
                }
            }
            let _scope = self._region.enter();
            let mut checker = self.program.get_type_checker(&Context::default());
            // the variables an object pattern on the left of `=` assigns a def to (`({ load } = api)`)
            let mut assigned: Vec<P<Symbol>> = Vec::new();
            let mut patterns: Vec<Span> = Vec::new();
            for &(left, right) in &body.assigns {
                patterns.push(own_span(func.file.node.text(), left));
                let ty = checker.get_type_at_location(right);
                self.assigned_defs(&mut checker, left, ty, style, &mut assigned);
            }
            // what no edge covers: the function behind a `bind`, a `call` or an `apply`, and behind an element access with a literal key
            for &target in &body.binds {
                if !self
                    .functions_named(&mut checker, target, style)
                    .0
                    .is_empty()
                {
                    uncovered.push(Uncovered::Bind);
                }
            }
            for &target in &body.call_apply {
                if !self
                    .functions_named(&mut checker, target, style)
                    .0
                    .is_empty()
                {
                    uncovered.push(Uncovered::CallApply);
                }
            }
            for &access in &body.elems {
                let key = access.as_element_access_expression().argument_expression;
                if ast::is_string_literal_like(key)
                    && !self.functions_named(&mut checker, key, style).0.is_empty()
                {
                    uncovered.push(Uncovered::ElementAccess);
                }
            }
            // a getter read by destructuring runs, and no property access is there to be its site
            for &element in &body.destructured {
                if bound_symbol(&mut checker, element).is_some_and(|symbol| {
                    symbol
                        .declarations()
                        .iter()
                        .any(|declaration| declaration.kind() == Kind::GetAccessor)
                }) {
                    uncovered.push(Uncovered::DestructuredAccessor);
                }
            }
            // in source order, as the body names them
            for &(span, node) in &body.refs {
                let symbol = if node.kind() == Kind::ShorthandPropertyAssignment {
                    checker.get_shorthand_assignment_value_symbol(Some(node))
                } else {
                    checker.get_symbol_at_location_exported(node)
                };
                let Some(mut symbol) = symbol else {
                    continue;
                };
                // a target of the pattern itself is where the def is taken, not where it is handed over
                if !patterns
                    .iter()
                    .any(|pattern| pattern.start <= span.start && span.end <= pattern.end)
                    && assigned.contains(&symbol)
                {
                    uncovered.push(Uncovered::AssignmentDestructuring);
                }
                if symbol.flags().intersects(SymbolFlags::Alias) {
                    symbol = checker.get_aliased_symbol(symbol);
                }
                // a name bound by destructuring (`const { load } = api`) stands for the member it names: the declarations of that member
                let mut declarations = Vec::new();
                for declaration in symbol.declarations().to_vec() {
                    if declaration.kind() == Kind::BindingElement {
                        // `...others`: the rest object holds the members the pattern did not name; any that is a def goes with it, and no edge says so
                        if declaration
                            .as_binding_element()
                            .dot_dot_dot_token()
                            .is_some()
                        {
                            let ty = checker.get_type_of_symbol(symbol);
                            if self.holds_defs(&mut checker, ty, style) {
                                uncovered.push(Uncovered::RestElement);
                            }
                        }
                        declarations.extend(bound_member(&mut checker, declaration));
                    } else {
                        declarations.push(declaration);
                    }
                }
                for declaration in declarations {
                    // a name that holds an object literal (`const local = { traverse: () => {} }`, a nested `local.data`) hands over that literal's
                    // function members, at the name
                    if (is_variable_like(Some(declaration))
                        || ast::is_property_assignment(declaration))
                        && let Some(literal) = declaration
                            .initializer()
                            .map(unwrap_expression)
                            .filter(|init| init.kind() == Kind::ObjectLiteralExpression)
                        && let Some(file_node) = ast::get_source_file_of_node(declaration)
                    {
                        let file = File {
                            node: file_node,
                            _sem: PhantomData,
                            _one_thread: PhantomData,
                        };
                        if self.is_project_code(file)
                            && let Some(defs) = self.file_defs(file, style)
                        {
                            let mut carried = Vec::new();
                            self.functions_of_literal(file, &defs, literal, &mut carried);
                            for (held, _) in carried {
                                let handed = Handed {
                                    func: held,
                                    written: true,
                                    at: Some(span),
                                };
                                if !found.contains(&handed) {
                                    found.push(handed);
                                }
                            }
                        }
                    }
                    let (functions, classes) =
                        self.functions_of_declaration(&mut checker, declaration, style);
                    for held in functions {
                        let handed = Handed {
                            func: held,
                            written: false,
                            at: Some(span),
                        };
                        // one edge for each place and def: the declarations of one symbol can name a def twice
                        if !found.contains(&handed) {
                            found.push(handed);
                        }
                    }
                    if classes > 0 {
                        uncovered.push(Uncovered::Class);
                    }
                }
            }
            HandOvers {
                handed: found,
                uncovered,
            }
        }

        /// Whether a member of `ty` is a project function with a body: the properties' own declarations, mapped as a call maps them.
        fn holds_defs(&self, checker: &mut Checker, ty: P<Type>, style: MarkerStyle<'_>) -> bool {
            let properties = checker.get_properties_of_type_exported(ty).to_vec();
            properties.into_iter().any(|property| {
                property
                    .declarations()
                    .to_vec()
                    .into_iter()
                    .any(|declaration| {
                        !self
                            .functions_of_declaration(checker, declaration, style)
                            .0
                            .is_empty()
                    })
            })
        }

        /// The variables that the object pattern `left` (the left of `=`) assigns a def of its value to: `load` in `({ load } = api)` and `({ load: go } = api)`,
        /// `run` in `({ inner: { run } } = deep)`, where the member of `ty` that the key names is a project function. A key that is not a name, a member the type
        /// does not have and a target that is not a variable are not followed.
        fn assigned_defs(
            &self,
            checker: &mut Checker,
            left: P<Node>,
            ty: P<Type>,
            style: MarkerStyle<'_>,
            out: &mut Vec<P<Symbol>>,
        ) {
            for &member in left.properties() {
                let Some(key) = member
                    .name()
                    .filter(|name| ast::is_identifier(*name) || ast::is_string_literal_like(*name))
                else {
                    continue;
                };
                let (target, nested) = match member.kind() {
                    Kind::ShorthandPropertyAssignment => (None, None),
                    Kind::PropertyAssignment => {
                        let value = member.initializer().map(unwrap_expression);
                        match value {
                            Some(value) if value.kind() == Kind::ObjectLiteralExpression => {
                                (None, Some(value))
                            }
                            Some(value) => (Some(value), None),
                            None => continue,
                        }
                    }
                    _ => continue,
                };
                let Some(property) = checker.get_property_of_type_exported(ty, key.text()) else {
                    continue;
                };
                if let Some(nested) = nested {
                    let inner = checker.get_type_of_symbol(property);
                    self.assigned_defs(checker, nested, inner, style, out);
                    continue;
                }
                let is_def = property
                    .declarations()
                    .to_vec()
                    .into_iter()
                    .any(|declaration| {
                        !self
                            .functions_of_declaration(checker, declaration, style)
                            .0
                            .is_empty()
                    });
                if !is_def {
                    continue;
                }
                let variable = match target {
                    None => checker.get_shorthand_assignment_value_symbol(Some(member)),
                    Some(target) => checker.get_symbol_at_location_exported(target),
                };
                if let Some(variable) = variable
                    && !out.contains(&variable)
                {
                    out.push(variable);
                }
            }
        }

        /// How many sites of `class`'s own body are in decorators: the decorators of the class, of its members and of the parameters of its constructor and
        /// methods, and the calls and accessors inside them (`@factory(1)` is two sites: the call of `factory`, and of its result).
        fn decorator_sites(&self, class: Func<'_>, style: MarkerStyle<'_>) -> usize {
            let Some(body) = self.body(class, style) else {
                return 0;
            };
            let text = class.file.node.text();
            let decorators: Vec<Span> = body
                .sites
                .iter()
                .filter(|site| site.kind == SiteKind::Decorator)
                .map(|site| own_span(text, site.call))
                .collect();
            body.sites
                .iter()
                .filter(|site| {
                    decorators
                        .iter()
                        .any(|d| d.start <= site.span.start && site.span.end <= d.end)
                })
                .count()
        }

        /// How many sites of `class`'s own body are in the initializers of its **instance** fields (`x = make()`): what a construction runs and does not
        /// draw. An arrow or a function expression held by a field is a def, so its calls are not in the class's body and not counted.
        pub(crate) fn initializer_sites(&self, class: Func<'_>, style: MarkerStyle<'_>) -> usize {
            let (Some(body), Some(defs)) =
                (self.body(class, style), self.file_defs(class.file, style))
            else {
                return 0;
            };
            let Some(node) = defs.nodes[class.def] else {
                return 0;
            };
            let text = class.file.node.text();
            let initializers: Vec<Span> = node
                .members()
                .iter()
                .filter(|member| {
                    member.kind() == Kind::PropertyDeclaration && !ast::is_static(**member)
                })
                .filter_map(|member| member.initializer())
                .map(|initializer| own_span(text, initializer))
                .collect();
            body.sites
                .iter()
                .filter(|site| {
                    initializers
                        .iter()
                        .any(|i| i.start <= site.span.start && site.span.end <= i.end)
                })
                .count()
        }

        /// Whether `class` has a `render` that is a project function with a body, its own or inherited from a class of the project: what a framework calls on
        /// a class component.
        pub(crate) fn has_render(&self, class: Func<'_>, style: MarkerStyle<'_>) -> bool {
            let Some(node) = self
                .file_defs(class.file, style)
                .and_then(|defs| defs.nodes[class.def])
            else {
                return false;
            };
            let _scope = self._region.enter();
            let mut checker = self.program.get_type_checker(&Context::default());
            let Some(symbol) = checker.get_symbol_of_declaration_exported(node) else {
                return false;
            };
            let ty = checker.get_declared_type_of_symbol_exported(symbol);
            let Some(render) = checker.get_property_of_type_exported(ty, "render") else {
                return false;
            };
            render
                .declarations()
                .to_vec()
                .into_iter()
                .any(|declaration| {
                    !self
                        .functions_of_declaration(&mut checker, declaration, style)
                        .0
                        .is_empty()
                })
        }

        /// The project functions with a body that `node` (a name, an access, a shorthand property) names, and how many project classes: the symbol's
        /// declarations, mapped as a call maps them (`callable_declarations`), kept if they are defs of the project.
        fn functions_named<'p>(
            &'p self,
            checker: &mut Checker,
            node: P<Node>,
            style: MarkerStyle<'_>,
        ) -> (Vec<Func<'p>>, usize) {
            let symbol = if node.kind() == Kind::ShorthandPropertyAssignment {
                checker.get_shorthand_assignment_value_symbol(Some(node))
            } else {
                checker.get_symbol_at_location_exported(node)
            };
            let Some(mut symbol) = symbol else {
                return (Vec::new(), 0);
            };
            if symbol.flags().intersects(SymbolFlags::Alias) {
                symbol = checker.get_aliased_symbol(symbol);
            }
            let mut functions = Vec::new();
            let mut classes = 0;
            for declaration in symbol.declarations().to_vec() {
                let (found, class_count) =
                    self.functions_of_declaration(checker, declaration, style);
                functions.extend(found);
                classes += class_count;
            }
            (functions, classes)
        }

        /// What one declaration is, as something a call would enter: the project functions with a body, and the number of project classes.
        fn functions_of_declaration<'p>(
            &'p self,
            checker: &mut Checker,
            declaration: P<Node>,
            style: MarkerStyle<'_>,
        ) -> (Vec<Func<'p>>, usize) {
            let mut functions = Vec::new();
            let mut classes = 0;
            for target in callable_declarations(checker, declaration) {
                let Some(file_node) = ast::get_source_file_of_node(target) else {
                    continue;
                };
                let file = File {
                    node: file_node,
                    _sem: PhantomData,
                    _one_thread: PhantomData,
                };
                if !self.is_project_code(file) {
                    continue;
                }
                let Some(defs) = self.file_defs(file, style) else {
                    continue;
                };
                let Some(&def) = defs.def_of.get(&target) else {
                    continue;
                };
                let held = Func { file, def };
                if self.is_function_with_body(held, style) {
                    functions.push(held);
                } else if defs.outline.defs[def].kind == DefKind::Class {
                    classes += 1;
                }
            }
            (functions, classes)
        }

        /// The function members of an object literal that are defs, through nested literals, each with the span of its key: a method, an arrow or a function
        /// expression held by a property (a def by D21's rule, or not: `def_of` says). A getter, a setter, a shorthand property (named in the literal, so a
        /// hand-over of its own where the literal is written) and a computed key are not looked at.
        fn functions_of_literal<'p>(
            &'p self,
            file: File<'p>,
            defs: &FileDefs,
            literal: P<Node>,
            out: &mut Vec<(Func<'p>, Span)>,
        ) {
            let text = file.node.text();
            for &member in literal.properties() {
                let key = member.name().map(|name| own_span(text, name));
                match member.kind() {
                    Kind::MethodDeclaration => {
                        if let (Some(&def), Some(key)) = (defs.def_of.get(&member), key) {
                            out.push((Func { file, def }, key));
                        }
                    }
                    Kind::PropertyAssignment => {
                        let Some(value) = member.initializer().map(unwrap_expression) else {
                            continue;
                        };
                        if value.kind() == Kind::ObjectLiteralExpression {
                            self.functions_of_literal(file, defs, value, out);
                        } else if let (Some(&def), Some(key)) = (defs.def_of.get(&value), key) {
                            out.push((Func { file, def }, key));
                        }
                    }
                    _ => {}
                }
            }
        }

        /// A def that is a function written with a body: a function declaration, a function expression, an arrow, a method. Not a class, an accessor,
        /// a constructor, or a signature (an interface's method, an ambient `declare function`: nothing to walk).
        fn is_function_with_body(&self, func: Func<'_>, style: MarkerStyle<'_>) -> bool {
            let Some(defs) = self.file_defs(func.file, style) else {
                return false;
            };
            defs.nodes[func.def].is_some_and(|node| {
                matches!(
                    node.kind(),
                    Kind::FunctionDeclaration
                        | Kind::FunctionExpression
                        | Kind::ArrowFunction
                        | Kind::MethodDeclaration
                ) && node.body().is_some()
            })
        }

        /// What the def is, in the core's terms: `Marked` by its marker when it has one, else named by where it is (the file relative to the root,
        /// its qualname, and which of the same-named defs of the file it is, in source order).
        pub(crate) fn describe(&self, func: Func<'_>, style: MarkerStyle<'_>) -> DefInfo {
            let defs = self
                .file_defs(func.file, style)
                .expect("a Func is made from an outline, so its file has one");
            let def = &defs.outline.defs[func.def];
            let file = self
                .relative_path(func.file)
                .unwrap_or_else(|| func.file.node.file_name().to_owned());
            let occurrence = defs.outline.defs[..func.def]
                .iter()
                .filter(|earlier| earlier.qualname == def.qualname)
                .count() as u16;
            let id = match &def.marker {
                Some(marker) => StableDef::Marked(marker.clone()),
                None => StableDef::Path {
                    module: file.clone(),
                    qname: def.qualname.clone(),
                    nth: occurrence,
                },
            };
            DefInfo::new(
                id,
                def.qualname.clone(),
                def.qualname.clone(),
                occurrence,
                def.kind.clone(),
                file,
                def.range,
            )
        }
    }

    /// The name of an external as the outline would qualify it: the names of the classes, interfaces and namespaces around the declaration, then its own
    /// (`Math.max`, `Array.map`, `helper`). Two methods of one package must not share a name: the merged wire counts a repeated target as one. A module
    /// whose name is a string (`declare module "x"`) adds nothing, since the package says it.
    fn qualified_name(declaration: P<Node>, own: String) -> String {
        let mut parts = vec![own];
        let mut parent = declaration.parent();
        while let Some(node) = parent {
            if matches!(
                node.kind(),
                Kind::ClassDeclaration
                    | Kind::ClassExpression
                    | Kind::InterfaceDeclaration
                    | Kind::ModuleDeclaration
            ) && let Some(name) = node.name().filter(|name| ast::is_identifier(*name))
            {
                parts.push(name.text().to_owned());
            }
            parent = node.parent();
        }
        parts.reverse();
        parts.join(".")
    }

    /// The declaration with a body among those of the symbol of an overload signature: the implementation, in the same file.
    fn implementation_of(checker: &mut Checker, signature: P<Node>) -> Option<P<Node>> {
        if !(ast::is_function_declaration(signature) || ast::is_method_declaration(signature)) {
            return None;
        }
        let file = ast::get_source_file_of_node(signature)?;
        symbol_of_declaration(checker, signature)?
            .declarations()
            .iter()
            .copied()
            .find(|declaration| {
                declaration.body().is_some()
                    && ast::get_source_file_of_node(*declaration) == Some(file)
                    && (ast::is_function_declaration(*declaration)
                        || ast::is_method_declaration(*declaration))
            })
    }

    /// The member a destructured name stands for: `load` in `const { load } = api` is `api`'s `load`, `inner` in `const { a: { inner } } = api` is the `inner` of
    /// `api.a`. The property is looked up on the type of the initializer, so a member the type does not have (an index signature, a rest element, a computed key,
    /// an array pattern, a parameter with no initializer) gives nothing: those are not followed.
    fn bound_symbol(checker: &mut Checker, element: P<Node>) -> Option<P<Symbol>> {
        let mut keys = Vec::new();
        let mut at = element;
        // from the element up through its patterns to the variable that is initialized
        let initializer = loop {
            let pattern = at.parent()?;
            if pattern.kind() != Kind::ObjectBindingPattern
                || at.as_binding_element().dot_dot_dot_token().is_some()
            {
                return None;
            }
            let key = at.property_name().or(at.name())?;
            if !(ast::is_identifier(key) || ast::is_string_literal_like(key)) {
                return None;
            }
            keys.push(key.text().to_string());
            let holder = pattern.parent()?;
            match holder.kind() {
                Kind::BindingElement => at = holder,
                Kind::VariableDeclaration => break holder.initializer()?,
                _ => return None,
            }
        };
        let mut ty = checker.get_type_at_location(initializer);
        let mut property = None;
        for key in keys.iter().rev() {
            let found = checker.get_property_of_type_exported(ty, key)?;
            ty = checker.get_type_of_symbol(found);
            property = Some(found);
        }
        property
    }

    /// The declarations of the member a destructured name stands for (`bound_symbol`).
    fn bound_member(checker: &mut Checker, element: P<Node>) -> Vec<P<Node>> {
        bound_symbol(checker, element)
            .map_or_else(Vec::new, |symbol| symbol.declarations().to_vec())
    }

    /// The accessors of the property at `location` that an access of kind `access` runs: the getters for a read, the setters for a write, both for a read and a write.
    /// From the symbol's own declarations, so a union's property (a synthetic symbol that holds every member's) gives every member's accessor, and a member that is a
    /// plain property gives none. `None` when there is no symbol.
    fn accessor_declarations(
        checker: &mut Checker,
        location: P<Node>,
        access: Access,
    ) -> Option<Vec<P<Node>>> {
        let symbol = checker.get_symbol_at_location_exported(location)?;
        if !symbol.flags().intersects(SymbolFlags::Accessor) {
            // a union's synthetic property carries the flags of its members; a plain symbol that is no accessor has nothing to run
            if symbol.declarations().iter().all(|declaration| {
                !matches!(declaration.kind(), Kind::GetAccessor | Kind::SetAccessor)
            }) {
                return Some(Vec::new());
            }
        }
        Some(
            symbol
                .declarations()
                .iter()
                .copied()
                .filter(|declaration| match declaration.kind() {
                    Kind::GetAccessor => access.reads(),
                    Kind::SetAccessor => access.writes(),
                    _ => false,
                })
                .collect(),
        )
    }

    /// Why the engine has no declaration for this callee: syntax first (a computed name), then the type the checker gives it.
    fn why_unknown(checker: &mut Checker, callee: P<Node>) -> Why {
        if ast::is_element_access_expression(callee) {
            return Why::ComputedName;
        }
        if checker
            .get_type_at_location(callee)
            .flags()
            .intersects(TypeFlags::Any)
        {
            Why::AnyOrUntyped
        } else if is_value_holder(checker, callee) {
            Why::FunctionTypedValue
        } else {
            Why::NoDeclaration
        }
    }

    /// Whether the callee names something that was **declared as a holder of a value**: a parameter, a property, a variable or a destructured binding
    /// (through an import alias too). That is what D16 means by a function-typed value, and it is the evidence behind the count: a callee with no
    /// symbol (`makeHandler()()`, `(a ? f : g)()`) or a symbol that is not one of these is `no_declaration`, so a function the resolver failed to
    /// enter cannot be counted as a D16 case just because every call that type-checks has a callable callee.
    fn is_value_holder(checker: &mut Checker, callee: P<Node>) -> bool {
        let Some(mut symbol) = checker.get_symbol_at_location_exported(callee) else {
            return false;
        };
        if symbol.flags().intersects(SymbolFlags::Alias) {
            symbol = checker.get_aliased_symbol(symbol);
        }
        let declarations = symbol.declarations();
        !declarations.is_empty()
            && declarations
                .iter()
                .all(|declaration| is_value_holder_declaration(*declaration))
    }

    /// A declaration that holds a value, whatever the value is: a parameter, a property, a variable, a destructured binding.
    fn is_value_holder_declaration(declaration: P<Node>) -> bool {
        matches!(
            declaration.kind(),
            Kind::Parameter
                | Kind::PropertySignature
                | Kind::PropertyDeclaration
                | Kind::VariableDeclaration
                | Kind::BindingElement
                | Kind::PropertyAssignment
                | Kind::ShorthandPropertyAssignment
        )
    }

    /// A callee that is a name: an identifier, a property access or an element access. For these the checker's resolved signature is the signature of
    /// the named function or method; for any other expression (a conditional, a `||`, a call's result) it is the signature of whichever type the
    /// expression reduced to.
    fn is_named_callee(callee: P<Node>) -> bool {
        matches!(
            callee.kind(),
            Kind::Identifier | Kind::PropertyAccessExpression | Kind::ElementAccessExpression
        )
    }

    /// Whether the callee's type is a union of more than one type once `undefined` and `null` are set aside: `(flag ? f : g)`. The checker's resolved
    /// signature names one declaration for such a call, which would pass for the whole answer.
    fn is_union_callee(checker: &mut Checker, callee: P<Node>) -> bool {
        let ty = checker.get_type_at_location(callee);
        ty.flags().intersects(TypeFlags::Union)
            && ty
                .types()
                .iter()
                .filter(|member| {
                    !member
                        .flags()
                        .intersects(TypeFlags::Undefined | TypeFlags::Null)
                })
                .count()
                > 1
    }

    // ---- the port of `tsrs_ls`'s `callhierarchy.rs`. Names and shapes follow the original so an engine bump can be checked line by line.

    /// A function or class expression with an identifier name.
    fn is_named_expression(node: P<Node>) -> bool {
        (ast::is_function_expression(node) || ast::is_class_expression(node))
            && node.name().is_some_and(ast::is_identifier)
    }

    /// An arrow or a function expression.
    fn is_function_value(node: P<Node>) -> bool {
        ast::is_arrow_function(node) || ast::is_function_expression(node)
    }

    /// An arrow or a function expression that is the value of a property of an object literal (`key: () => {}`).
    fn is_property_function_value(node: P<Node>) -> bool {
        is_function_value(node) && node.parent().is_some_and(ast::is_property_assignment)
    }

    fn is_variable_like(node: Option<P<Node>>) -> bool {
        node.is_some_and(|node| {
            ast::is_property_declaration(node) || ast::is_variable_declaration(node)
        })
    }

    /// A function, arrow or class expression assigned to a constant variable or class property.
    fn is_assigned_expression(node: P<Node>) -> bool {
        if !(ast::is_function_expression(node)
            || ast::is_arrow_function(node)
            || ast::is_class_expression(node))
        {
            return false;
        }
        if node.name().is_some() {
            return false;
        }
        let parent = node.parent();
        if !is_variable_like(parent) {
            return false;
        }
        let parent = parent.expect("is_variable_like says there is one");
        if parent.initializer() != Some(node) {
            return false;
        }
        if !parent.name().is_some_and(ast::is_identifier) {
            return false;
        }
        ast::get_combined_node_flags(parent).intersects(NodeFlags::Const)
            || ast::is_property_declaration(parent)
    }

    fn is_possible_call_hierarchy_declaration(node: Option<P<Node>>) -> bool {
        node.is_some_and(|node| {
            ast::is_source_file(node)
                || ast::is_module_declaration(node)
                || ast::is_function_declaration(node)
                || ast::is_function_expression(node)
                || ast::is_class_declaration(node)
                || ast::is_class_expression(node)
                || ast::is_class_static_block_declaration(node)
                || ast::is_method_declaration(node)
                || ast::is_method_signature_declaration(node)
                || ast::is_get_accessor_declaration(node)
                || ast::is_set_accessor_declaration(node)
        })
    }

    fn is_valid_call_hierarchy_declaration(node: Option<P<Node>>) -> bool {
        let Some(node) = node else {
            return false;
        };
        if ast::is_source_file(node) {
            return true;
        }
        if ast::is_module_declaration(node) {
            return node.name().is_some_and(ast::is_identifier);
        }
        ast::is_function_declaration(node)
            || ast::is_class_declaration(node)
            || ast::is_class_static_block_declaration(node)
            || ast::is_method_declaration(node)
            || ast::is_method_signature_declaration(node)
            || ast::is_get_accessor_declaration(node)
            || ast::is_set_accessor_declaration(node)
            || is_named_expression(node)
            || is_assigned_expression(node)
    }

    /// The node that stands for a declaration when asking the checker about it.
    fn reference_node(node: P<Node>) -> Option<P<Node>> {
        if ast::is_source_file(node) {
            return Some(node);
        }
        if let Some(name) = node.name() {
            return Some(name);
        }
        if is_assigned_expression(node) {
            return node.parent().and_then(|parent| parent.name());
        }
        node.modifiers().and_then(|modifiers| {
            modifiers
                .nodes()
                .iter()
                .copied()
                .find(|modifier| modifier.kind() == Kind::DefaultKeyword)
        })
    }

    fn symbol_of_declaration(checker: &mut Checker, node: P<Node>) -> Option<P<Symbol>> {
        if ast::is_class_static_block_declaration(node) {
            return None;
        }
        checker.get_symbol_at_location_exported(reference_node(node)?)
    }

    /// The implementation of an overloaded function, method or constructor: the declaration with a body.
    fn find_implementation(checker: &mut Checker, node: P<Node>) -> Option<P<Node>> {
        if !ast::is_function_like_declaration(Some(node)) || node.body().is_some() {
            return Some(node);
        }
        if ast::is_constructor_declaration(node) {
            return ast::get_first_constructor_with_body(node.parent()?);
        }
        if ast::is_function_declaration(node) || ast::is_method_declaration(node) {
            let value_declaration =
                symbol_of_declaration(checker, node).and_then(|symbol| symbol.value_declaration());
            return value_declaration.filter(|declaration| {
                ast::is_function_like_declaration(Some(*declaration))
                    && declaration.body().is_some()
            });
        }
        Some(node)
    }

    /// Every declaration of the symbol that is itself a call hierarchy declaration, in (file, offset) order, with adjacent overload signatures of
    /// one parent counted once.
    fn find_all_initial_declarations(checker: &mut Checker, node: P<Node>) -> Vec<P<Node>> {
        let Some(symbol) = symbol_of_declaration(checker, node) else {
            return Vec::new();
        };
        let mut declarations: Vec<P<Node>> = symbol.declarations().to_vec();
        declarations.sort_by_key(|declaration| {
            (
                ast::get_source_file_of_node(*declaration).map(|file| file.file_name().to_owned()),
                declaration.pos(),
            )
        });
        let mut found: Vec<P<Node>> = Vec::new();
        let mut last: Option<P<Node>> = None;
        for declaration in declarations {
            if is_valid_call_hierarchy_declaration(Some(declaration)) {
                let separate = last.is_none_or(|last| {
                    last.parent() != declaration.parent() || last.end() != declaration.pos()
                });
                if separate {
                    found.push(declaration);
                }
                last = Some(declaration);
            }
        }
        found
    }

    fn find_implementation_or_all_initial_declarations(
        checker: &mut Checker,
        node: P<Node>,
    ) -> Vec<P<Node>> {
        if ast::is_class_static_block_declaration(node) {
            return vec![node];
        }
        if ast::is_function_like_declaration(Some(node)) {
            if let Some(implementation) = find_implementation(checker, node) {
                return vec![implementation];
            }
            let all = find_all_initial_declarations(checker, node);
            return if all.is_empty() { vec![node] } else { all };
        }
        let all = find_all_initial_declarations(checker, node);
        if all.is_empty() { vec![node] } else { all }
    }

    /// A call on a union (`w.write()` with `w: JsonWriter | CsvWriter`): the checker gives the property of a union a synthetic symbol that holds the
    /// declarations of **every** member (`checker_11.rs`, `create_union_or_intersection_property`) and sets its value declaration only when the members
    /// share one. The engine's call hierarchy follows the value declaration alone, so it finds nothing; the declarations are asked here instead. They
    /// are the answer for `a.b` and for `a["b"]` with a literal name (the symbol of the literal), and `undefined` in the union adds nothing, since the
    /// checker's symbol is the property's, not the type's. Returns the targets in (file, offset) order and whether every declaration was one that
    /// can be entered; `None` when the callee has no such symbol (the declarations are not a union's, or there is a value declaration to follow).
    fn declarations_of_union_symbol(
        checker: &mut Checker,
        callee: P<Node>,
    ) -> Option<(Vec<P<Node>>, bool)> {
        let location = if ast::is_property_access_expression(callee) {
            callee
        } else if ast::is_element_access_expression(callee) {
            let argument = callee.as_element_access_expression().argument_expression;
            ast::is_string_literal_like(argument).then_some(argument)?
        } else {
            return None;
        };
        let symbol = checker.get_symbol_at_location_exported(location)?;
        if symbol.value_declaration().is_some() || symbol.declarations().is_empty() {
            return None;
        }
        let mut found: Vec<P<Node>> = Vec::new();
        let mut all_resolved = true;
        for &declaration in symbol.declarations() {
            let targets = callable_declarations(checker, declaration);
            all_resolved &= !targets.is_empty();
            for target in targets {
                if !found.contains(&target) {
                    found.push(target);
                }
            }
        }
        found.sort_by_key(|declaration| {
            (
                ast::get_source_file_of_node(*declaration).map(|file| file.file_name().to_owned()),
                declaration.pos(),
            )
        });
        Some((found, all_resolved))
    }

    /// What a declaration of a property or method is, as something to enter: a callable declaration (its implementation, or all its initial
    /// declarations), or the arrow or function a property or variable holds (`write = () => {}`), which is what the engine does for a declaration
    /// name. Empty when it is neither: a method signature of a function type, a property of type `any`.
    fn callable_declarations(checker: &mut Checker, declaration: P<Node>) -> Vec<P<Node>> {
        if is_valid_call_hierarchy_declaration(Some(declaration)) {
            return find_implementation_or_all_initial_declarations(checker, declaration);
        }
        if is_variable_like(Some(declaration))
            && let Some(initializer) = declaration
                .initializer()
                .filter(|initializer| is_assigned_expression(*initializer))
        {
            return vec![initializer];
        }
        // the same addition as in `declarations_of` (D21): a member of a union that holds an arrow in an object literal
        if ast::is_property_assignment(declaration)
            && let Some(initializer) = declaration
                .initializer()
                .filter(|initializer| is_function_value(*initializer))
        {
            return vec![initializer];
        }
        Vec::new()
    }

    /// What the checker's resolved signature says, for a call the hierarchy's rule finds nothing for (`counter["bump"]()`, whose callee is not a name
    /// the hierarchy can follow, but whose method the checker knows). Only a declaration that is itself callable counts: a function **type** (a callback
    /// parameter) has a signature and no declaration to enter.
    fn declarations_of_signature(checker: &mut Checker, call: P<Node>) -> Vec<P<Node>> {
        let signature = checker.get_resolved_signature_exported(call);
        match signature.declaration() {
            Some(declaration) if is_valid_call_hierarchy_declaration(Some(declaration)) => {
                find_implementation_or_all_initial_declarations(checker, declaration)
            }
            // `api["load"]()`: the signature of an arrow held by a property of an object literal (D21)
            Some(declaration) if is_property_function_value(declaration) => vec![declaration],
            _ => Vec::new(),
        }
    }

    /// The declarations the engine's call hierarchy gives for the callee expression `location`; empty when it gives none.
    fn declarations_of(checker: &mut Checker, location: P<Node>) -> Vec<P<Node>> {
        let mut following_symbol = false;
        let mut location = location;
        loop {
            if is_valid_call_hierarchy_declaration(Some(location)) {
                return find_implementation_or_all_initial_declarations(checker, location);
            }
            if is_possible_call_hierarchy_declaration(Some(location))
                && let Some(ancestor) = ast::find_ancestor(Some(location), |n| {
                    is_valid_call_hierarchy_declaration(Some(n))
                })
            {
                return find_implementation_or_all_initial_declarations(checker, ancestor);
            }
            if ast::is_declaration_name(location) {
                let parent = location.parent();
                if is_valid_call_hierarchy_declaration(parent) {
                    return find_implementation_or_all_initial_declarations(
                        checker,
                        parent.expect("valid says there is one"),
                    );
                }
                if is_possible_call_hierarchy_declaration(parent)
                    && let Some(ancestor) =
                        ast::find_ancestor(parent, |n| is_valid_call_hierarchy_declaration(Some(n)))
                {
                    return find_implementation_or_all_initial_declarations(checker, ancestor);
                }
                if is_variable_like(parent)
                    && let Some(initializer) = parent
                        .and_then(|parent| parent.initializer())
                        .filter(|initializer| is_assigned_expression(*initializer))
                {
                    return vec![initializer];
                }
                return Vec::new();
            }
            if ast::is_constructor_declaration(location) {
                return match location.parent() {
                    Some(class) if is_valid_call_hierarchy_declaration(Some(class)) => vec![class],
                    _ => Vec::new(),
                };
            }
            if location.kind() == Kind::StaticKeyword
                && let Some(block) = location
                    .parent()
                    .filter(|parent| ast::is_class_static_block_declaration(*parent))
            {
                location = block;
                continue;
            }
            if ast::is_variable_declaration(location)
                && let Some(initializer) = location
                    .initializer()
                    .filter(|initializer| is_assigned_expression(*initializer))
            {
                return vec![initializer];
            }
            if !following_symbol
                && let Some(mut symbol) = checker.get_symbol_at_location_exported(location)
            {
                if symbol.flags().intersects(SymbolFlags::Alias) {
                    symbol = checker.get_aliased_symbol(symbol);
                }
                if let Some(value_declaration) = symbol.value_declaration() {
                    following_symbol = true;
                    location = value_declaration;
                    continue;
                }
            }
            return Vec::new();
        }
    }
}

pub(crate) use resolve::{Handed, Init, Resolved, Uncovered};

#[cfg(test)]
mod tests {
    use super::*;
    use typescript_syntax::MarkerStyle;

    fn three_files() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/three-files")
    }

    #[test]
    fn the_program_has_one_checker() {
        let sem = Sem::open(&three_files(), Path::new("src/main.ts")).unwrap();
        assert_eq!(sem.program.options().checkers, Some(1));
    }

    #[test]
    fn the_program_lists_its_files_in_the_same_order_every_time() {
        // Files are parsed on several threads, so the order the program ends with must not depend on which finished first.
        let names = || -> Vec<String> {
            let sem = Sem::open(&three_files(), Path::new("src/main.ts")).unwrap();
            sem.program
                .get_source_files()
                .iter()
                .map(|file| file.file_name().to_owned())
                .collect()
        };
        let first = names();
        assert!(
            first.len() > 3,
            "the bundled libraries are in the program too"
        );
        for _ in 0..30 {
            assert_eq!(names(), first);
        }
    }

    fn with_node_modules() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/with-node-modules")
    }

    /// (relative path or bundled, is project code, package) of every file in the program, sorted.
    fn classified(sem: &Sem) -> Vec<(String, bool, Option<String>)> {
        let mut all: Vec<_> = sem
            .files()
            .map(|file| {
                (
                    sem.relative_path(file)
                        .unwrap_or_else(|| "(outside root)".into()),
                    sem.is_project_code(file),
                    sem.package_of(file),
                )
            })
            .collect();
        all.sort();
        all
    }

    #[test]
    fn project_code_is_told_apart_from_packages_declarations_and_libraries() {
        let sem = Sem::open(&with_node_modules(), Path::new("src/main.ts")).unwrap();
        let all = classified(&sem);
        let under_root: Vec<_> = all
            .iter()
            .filter(|(path, ..)| path != "(outside root)")
            .collect();
        let row = |path: &str, project: bool, package: Option<&str>| {
            (path.to_owned(), project, package.map(str::to_owned))
        };
        assert_eq!(
            under_root,
            [
                &row(
                    "node_modules/@scope/other/index.d.ts",
                    false,
                    Some("@scope/other")
                ),
                &row("node_modules/@types/fake/index.d.ts", false, Some("fake")),
                &row("node_modules/pkg/index.d.ts", false, Some("pkg")),
                &row("src/main.ts", true, None),
                &row("src/types.d.ts", false, None),
            ]
        );
        // everything else is a bundled library: not project code, one package for all of them
        let libs: Vec<_> = all
            .iter()
            .filter(|(path, ..)| path == "(outside root)")
            .collect();
        assert!(libs.len() > 50, "{} libraries", libs.len());
        assert!(
            libs.iter()
                .all(|(_, project, package)| !project
                    && package.as_deref() == Some("typescript:lib"))
        );
    }

    #[test]
    fn a_repo_declaration_file_is_in_the_program_but_is_not_project_code() {
        let sem = Sem::open(&with_node_modules(), Path::new("src/main.ts")).unwrap();
        let types = sem
            .file("src/types.d.ts")
            .expect("main.ts imports it, so the program holds it");
        assert!(!sem.is_project_code(types));
        assert_eq!(sem.package_of(types), None);
    }

    #[test]
    fn an_imported_json_file_is_held_by_the_program_and_is_not_project_code() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json-import");
        let sem = Sem::open(&root, Path::new("src/main.ts")).unwrap();
        let data = sem.file("src/data.json").expect("main.ts imports it");
        assert!(!sem.is_project_code(data));
        assert!(!sem.is_repo_declaration_file(data));
    }

    #[test]
    fn a_file_is_found_by_its_relative_path() {
        let sem = Sem::open(&with_node_modules(), Path::new("src/main.ts")).unwrap();
        let file = sem.file("./src/../src/main.ts").unwrap();
        assert_eq!(sem.relative_path(file), Some("src/main.ts".into()));
        assert!(sem.file("src/gone.ts").is_none());
    }

    #[test]
    fn a_windows_verbatim_path_is_spelled_the_way_the_engine_reads_it() {
        let engine = |path: &str| to_engine_path(Path::new(path));
        assert_eq!(engine(r"\\?\D:\repo\src\main.ts"), "D:/repo/src/main.ts");
        assert_eq!(
            engine(r"\\?\UNC\server\share\main.ts"),
            "//server/share/main.ts"
        );
        // already ordinary: unchanged but for the separator
        assert_eq!(engine(r"D:\repo\main.ts"), "D:/repo/main.ts");
        assert_eq!(engine("/repo/src/main.ts"), "/repo/src/main.ts");
    }

    #[test]
    fn a_directory_is_within_the_root_whichever_way_each_is_spelled() {
        // the root is canonical, so on Windows verbatim; the directory of the climb is an engine path. Text on both sides, so this runs on every OS.
        assert!(is_within(
            Path::new(r"\\?\D:\repo"),
            Path::new("D:/repo/sub")
        ));
        assert!(is_within(Path::new(r"\\?\D:\repo"), Path::new("D:/repo")));
        assert!(is_within(Path::new(r"D:\repo"), Path::new("D:/repo/a/b")));
        assert!(is_within(Path::new("/repo"), Path::new("/repo/sub")));
        assert!(is_within(
            Path::new(r"\\?\UNC\server\share"),
            Path::new("//server/share/sub")
        ));
        // above the root, beside it, and a name that only starts with the root's
        assert!(!is_within(Path::new(r"\\?\D:\repo"), Path::new("D:/")));
        assert!(!is_within(Path::new(r"\\?\D:\repo"), Path::new("D:/other")));
        assert!(!is_within(
            Path::new(r"\\?\D:\repo"),
            Path::new("D:/repository")
        ));
        assert!(!is_within(Path::new("/repo"), Path::new("/repository")));
    }

    #[test]
    fn a_package_is_named_from_the_last_node_modules_in_the_path() {
        let name = |path: &str| package_in_node_modules(path);
        assert_eq!(
            name("/r/node_modules/lodash/fp/index.js"),
            Some("lodash".into())
        );
        assert_eq!(
            name("/r/node_modules/@scope/pkg/a.js"),
            Some("@scope/pkg".into())
        );
        assert_eq!(
            name("/r/node_modules/@types/node/fs.d.ts"),
            Some("node".into())
        );
        assert_eq!(
            name("/r/node_modules/@types/babel__core/i.d.ts"),
            Some("@babel/core".into())
        );
        assert_eq!(
            name("/r/node_modules/.pnpm/react@18/node_modules/react/index.js"),
            Some("react".into())
        );
        assert_eq!(name("/r/src/node_modules_like/a.ts"), None);
        assert_eq!(name("/r/node_modules/"), None);
    }

    const STYLE: MarkerStyle<'static> = MarkerStyle {
        label: "ID",
        placement: typescript_syntax::Placement::Docstring,
    };

    fn entries() -> Sem {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/entries");
        Sem::open(&root, Path::new("src/main.ts")).unwrap()
    }

    /// (qualname, start of the def in bytes) of the def `entry` finds.
    fn found(sem: &Sem, qualname: &str, occurrence: u16) -> Option<(String, u32)> {
        let func = sem.entry("src/main.ts", qualname, occurrence, STYLE)?;
        let def = sem.def(func, STYLE);
        Some((def.qualname.clone(), def.range.start))
    }

    #[test]
    fn entry_finds_the_nth_def_with_a_qualname_in_source_order() {
        let sem = entries();
        // overloads with an implementation are one def: `dup` has one
        assert!(found(&sem, "dup", 0).is_some());
        assert_eq!(found(&sem, "dup", 1), None);
        // signatures with no implementation are a def each, with one qualname
        let first = found(&sem, "ambient", 0).unwrap().1;
        let second = found(&sem, "ambient", 1).unwrap().1;
        assert!(first < second);
        assert_eq!(found(&sem, "ambient", 2), None);
        // a getter and a setter of one property
        let getter = found(&sem, "Box.size", 0).unwrap().1;
        let setter = found(&sem, "Box.size", 1).unwrap().1;
        assert!(getter < setter);
        assert_eq!(found(&sem, "Box.size", 2), None);
    }

    #[test]
    fn entry_finds_every_kind_of_def_by_its_qualname() {
        let sem = entries();
        for qualname in ["Box", "Box.open", "arrow", "outer", "outer.<locals>.inner"] {
            assert_eq!(found(&sem, qualname, 0).unwrap().0, qualname);
        }
    }

    #[test]
    fn entry_is_none_for_an_unknown_qualname_or_file() {
        let sem = entries();
        assert_eq!(found(&sem, "nope", 0), None);
        assert!(sem.entry("src/gone.ts", "dup", 0, STYLE).is_none());
        // `inner` alone is not its qualname
        assert_eq!(found(&sem, "inner", 0), None);
    }

    #[test]
    fn a_def_of_a_file_that_is_not_code_cannot_be_an_entry() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json-import");
        let sem = Sem::open(&root, Path::new("src/main.ts")).unwrap();
        assert!(sem.entry("src/data.json", "items", 0, STYLE).is_none());
    }

    /// Every file of every fixture the TS1 outline suite has (it covers every kind of def), opened as a project of its own: the root is the
    /// dialect's folder, which has no config above it, so each file is an inferred project.
    fn outline_fixtures() -> Vec<(String, PathBuf, PathBuf)> {
        let base =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../typescript-syntax/tests/fixtures");
        let mut found = Vec::new();
        for dialect in ["ts", "tsx", "js", "jsx"] {
            let root = base.join(dialect);
            let mut names: Vec<_> = std::fs::read_dir(&root)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            names.sort();
            for name in names {
                found.push((
                    format!("{dialect}/{}", name.to_string_lossy()),
                    root.clone(),
                    PathBuf::from(name),
                ));
            }
        }
        found
    }

    #[test]
    fn every_def_of_every_fixture_maps_to_exactly_one_program_node_and_back() {
        let mut checked_files = 0;
        let mut checked_defs = 0;
        let mut problems = Vec::new();
        for (label, root, file) in outline_fixtures() {
            let sem = Sem::open(&root, &file)
                .unwrap_or_else(|error| panic!("{label} does not open: {error}"));
            for held in sem.files() {
                // the files of the fixture folder (not the bundled libraries): a file the entry imports is checked too
                let Some(relative) = sem.relative_path(held) else {
                    continue;
                };
                let Some(defs) = sem.file_defs(held, STYLE) else {
                    continue;
                };
                checked_files += 1;
                let mut seen = HashSet::new();
                for (index, def) in defs.outline.defs.iter().enumerate() {
                    checked_defs += 1;
                    let Some(node) = defs.nodes[index] else {
                        problems.push(format!(
                            "{label} {relative}: `{}` has no node",
                            def.qualname
                        ));
                        continue;
                    };
                    if !seen.insert(node) {
                        problems.push(format!(
                            "{label} {relative}: `{}` shares its node",
                            def.qualname
                        ));
                    }
                    if defs.def_of.get(&node) != Some(&index) {
                        problems.push(format!(
                            "{label} {relative}: `{}` does not map back",
                            def.qualname
                        ));
                    }
                }
                if defs.def_of.len() != defs.outline.defs.len() {
                    problems.push(format!(
                        "{label} {relative}: {} nodes for {} defs",
                        defs.def_of.len(),
                        defs.outline.defs.len()
                    ));
                }
            }
        }
        assert!(
            problems.is_empty(),
            "{} problems:\n{}",
            problems.len(),
            problems.join("\n")
        );
        // not a vacuous pass: every fixture file was checked, and they hold a good many defs
        assert_eq!(checked_files, outline_fixtures().len());
        assert!(checked_defs > 250, "{checked_defs} defs");
    }

    fn sites_project() -> Sem {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sites");
        Sem::open(&root, Path::new("src/sites.ts")).unwrap()
    }

    /// The callee text of every site of every def of `src/sites.ts` that has one, by qualname.
    fn sites_by_def(sem: &Sem) -> std::collections::BTreeMap<String, Vec<String>> {
        let file = sem.file("src/sites.ts").unwrap();
        let text = file.node.text();
        let defs = sem.file_defs(file, STYLE).unwrap();
        let mut by_def = std::collections::BTreeMap::new();
        for (index, def) in defs.outline.defs.iter().enumerate() {
            let func = Func { file, def: index };
            let sites = sem
                .call_sites(func, STYLE)
                .expect("every def of this file has a node");
            let texts: Vec<String> = sites
                .iter()
                .map(|span| span.slice(text).unwrap().to_owned())
                .collect();
            if !texts.is_empty() {
                by_def.insert(def.qualname.clone(), texts);
            }
        }
        by_def
    }

    /// The text of every site of the def `qualname` of `src/accessors.ts` in the `site-kinds` fixture, in order.
    fn accessor_sites_of(qualname: &str) -> Vec<String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/site-kinds");
        let sem = Sem::open(&root, Path::new("src/accessors.ts")).unwrap();
        let func = sem.entry("src/accessors.ts", qualname, 0, STYLE).unwrap();
        let text = func.file.node.text();
        sem.call_sites(func, STYLE)
            .unwrap()
            .iter()
            .map(|span| span.slice(text).unwrap().to_owned())
            .collect()
    }

    #[test]
    fn a_property_access_that_is_a_callee_is_one_site_and_not_also_an_accessor_site() {
        // two sites on one span would resolve to the same edge and be merged by the graph, so only the sites of the body can tell
        assert_eq!(accessor_sites_of("callee"), ["box.method", "box.handler"]);
        // each access of a chain is a site; the callee-position test looks through parentheses and assertions
        assert_eq!(accessor_sites_of("chain"), ["box.inner", "box.inner.value"]);
        // a field, a method read and not called, `delete` and a computed key are no sites
        assert!(accessor_sites_of("none").is_empty());
    }

    #[test]
    fn a_body_s_plain_calls_are_its_sites_in_source_order() {
        let found = sites_by_def(&sites_project());
        let expect = |qualname: &str, sites: &[&str]| {
            assert_eq!(
                found
                    .get(qualname)
                    .map(|v| v.iter().map(String::as_str).collect::<Vec<_>>()),
                Some(sites.to_vec()),
                "{qualname}"
            );
        };
        // the callee expression is the site: not the arguments, not the type arguments
        expect(
            "plain",
            &["f", "a.b", "a?.b", "obj.x.y", "a[\"key\"]", "(g)"],
        );
        // by where the callee starts, then where it ends; an argument's call follows its callee's
        expect(
            "ordering",
            &["f", "g", "f", "f()", "g", "a.b", "a.b(f()).c", "f"],
        );
        expect(
            "wrappers",
            &["(f() as any).m", "f", "f", "f()!.n", "f", "g"],
        );
        // a parameter's default value and a destructuring default are part of the body
        expect("defaults", &["f", "g"]);
        // an anonymous function is part of the def around it
        expect("callbacks", &["xs.map", "g", "xs.forEach", "f", "f"]);
        // a named nested function and a const arrow are defs of their own
        expect("nested", &["f"]);
        expect("nested.<locals>.inner", &["g"]);
        expect("nested.<locals>.arrow", &["a.b"]);
        // `new Base()` is a site on the class expression, a tagged template on its tag; `import()` and `require()` are never, and `a.getterOnly` is one only when the
        // checker says it is an accessor (`a` is `any` here)
        expect("not_sites", &["Base", "f"]);
        // a class: `extends`, a property initializer, a static property, a static block and a decorator are its body; its methods are not
        // `@dec(1)` is the call of `dec` and, with the whole decorator expression, the call through its result
        expect("Widget", &["mixin", "f", "g", "a.init", "dec", "dec(1)"]);
        expect("Widget.method", &["f"]);
        expect("Widget.value", &["g"]);
        expect("Widget.constructor", &["super", "f"]);
        assert_eq!(found.get("Widget.decorated"), None);
        // an object-literal method is a def; a named function expression is a def; overloads have one body
        expect("table.load", &["f"]);
        // an arrow held by a property of a `const` object literal is a def too (D21): its call is its own, no longer nobody's
        expect("table.size", &["g"]);
        expect("expression_names", &["[1, 2].map"]);
        expect("expression_names.<locals>.named", &["g"]);
        expect("overloaded", &["f"]);
        expect("arrow_def", &["f", "g"]);
        // a namespace's body is its own (the code that runs when the namespace is first evaluated); the defs inside it are their own
        expect("Ns", &["f"]);
        expect("Ns.in_namespace", &["g"]);
        // a nested class is a def, but the calls in its computed member names belong to the function around it (as in tsrs)
        expect("holder", &["f"]);
        assert_eq!(found.get("holder.<locals>.Local"), None);
        // the type side of `satisfies` is not walked, the expression is
        expect("satisfying", &["(f() satisfies any).m", "f"]);
        // ambient code in a body is a declaration, not a call
        expect("ambient_in_body", &["g"]);
        // nothing else has a site: an interface method, an ambient `declare`, a leaf
        let listed = [
            "plain",
            "ordering",
            "wrappers",
            "defaults",
            "callbacks",
            "nested",
            "nested.<locals>.inner",
            "nested.<locals>.arrow",
            "not_sites",
            "Widget",
            "Widget.method",
            "Widget.value",
            "Widget.constructor",
            "table.load",
            "table.size",
            "expression_names",
            "expression_names.<locals>.named",
            "overloaded",
            "arrow_def",
            "Ns",
            "Ns.in_namespace",
            "holder",
            "satisfying",
            "ambient_in_body",
        ];
        let others: Vec<_> = found
            .keys()
            .filter(|q| !listed.contains(&q.as_str()))
            .collect();
        assert!(others.is_empty(), "{others:?}");
    }

    #[test]
    fn a_files_defs_are_made_once_per_marker_style() {
        let sem = entries();
        let file = sem.file("src/main.ts").unwrap();
        let first = sem.file_defs(file, STYLE).unwrap();
        let again = sem.file_defs(file, STYLE).unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        // a marker is read by its label, so another label is another outline
        let other = sem.file_defs(file, MarkerStyle::docstring("REF")).unwrap();
        assert!(!Arc::ptr_eq(&first, &other));
        assert!(Arc::ptr_eq(
            &other,
            &sem.file_defs(file, MarkerStyle::docstring("REF")).unwrap()
        ));
    }

    #[test]
    fn a_def_is_read_in_place_not_copied() {
        let sem = entries();
        let func = sem.entry("src/main.ts", "Box.open", 0, STYLE).unwrap();
        let def = sem.def(func, STYLE);
        assert_eq!(def.qualname, "Box.open");
        // two reads share the one outline
        let again = sem.def(func, STYLE);
        assert!(std::ptr::eq(&*def, &*again));
    }

    /// What the engine says each site of `callers` in `tests/fixtures/resolve` calls, as `file qualname`, `external package name` or `unknown why`.
    /// Written from the policy table (`docs/03-typescript/06-ts4-calls.md` §5) and the engine's own rule before the run.
    fn resolved_sites(sem: &Sem, file: &str, qualname: &str) -> Vec<(String, Vec<String>)> {
        let source = sem.file(file).unwrap();
        let text = source.node.text();
        let func = sem.entry(file, qualname, 0, STYLE).unwrap();
        let spans = sem.call_sites(func, STYLE).unwrap();
        (0..spans.len())
            .map(|index| {
                let described = sem
                    .callee(func, index, STYLE)
                    .into_iter()
                    .map(|resolved| match resolved {
                        Resolved::Def(target) => {
                            let info = sem.describe(target, STYLE);
                            // the nth of a def that shares its qualname (merged declarations) is shown, as `#n`, when it is not the first
                            match info.id {
                                analyzer_core::graph::StableDef::Path { nth, .. } if nth > 0 => {
                                    format!("{} {}#{nth}", info.file, info.qname)
                                }
                                _ => format!("{} {}", info.file, info.qname),
                            }
                        }
                        Resolved::External { package, name } => {
                            format!("external {package} {name}")
                        }
                        Resolved::Unknown(why) => format!("unknown {}", why.counter()),
                    })
                    .collect();
                (spans[index].slice(text).unwrap().to_owned(), described)
            })
            .collect()
    }

    #[test]
    fn sem_contract_each_site_resolves_as_the_policy_table_says() {
        let sem = Sem::open(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolve"),
            Path::new("src/main.ts"),
        )
        .unwrap();
        let found = resolved_sites(&sem, "src/main.ts", "callers");
        let one = |site: &str, target: &str| (site.to_owned(), vec![target.to_owned()]);
        let expected = vec![
            one("plain", "src/lib.ts plain"),
            one("counter.bump", "src/lib.ts Counter.bump"),
            one("Counter.make", "src/lib.ts Counter.make"),
            // an interface method: the signature, a leaf with no body, not the implementations
            one("writer.write", "src/lib.ts Writer.write"),
            // a union receiver: one target per member that declares the method (policy table). The engine's call hierarchy gives none and the
            // checker's signature gives only the first member, so the members are asked one by one
            (
                "either.write".to_owned(),
                vec![
                    "src/lib.ts JsonWriter.write".to_owned(),
                    "src/lib.ts CsvWriter.write".to_owned(),
                ],
            ),
            // overloads: the implementation, which is the one def
            one("overloaded", "src/lib.ts overloaded"),
            one("generic", "src/lib.ts generic"),
            one("fallback", "src/lib.ts fallback"),
            one("arrow", "src/lib.ts arrow"),
            // followed through `export *`, a namespace import and a renamed import to the real declaration
            one("reexported", "src/more.ts reexported"),
            one("lib.plain", "src/lib.ts plain"),
            one("renamed", "src/lib.ts plain"),
            // outside project code: a package, the repo's own `.d.ts` (named by its path), the bundled libraries
            one("helper", "external pkg helper"),
            one("declared", "external src/types.d.ts declared"),
            one("Math.max", "external typescript:lib Math.max"),
            one("[1].map", "external typescript:lib Array.map"),
            // the engine has no declaration to enter, and says why
            one("cb", "unknown function_typed_value"),
            one("anyValue.run", "unknown any_or_untyped"),
            one("anyValue", "unknown any_or_untyped"),
            one("(lib as any)[key]", "unknown computed_name"),
            // a computed name the checker narrows to a literal is resolved
            one("counter[\"bump\"]", "src/lib.ts Counter.bump"),
            // a union through an element access with a literal name: the members are asked the same way
            (
                "either[\"write\"]".to_owned(),
                vec![
                    "src/lib.ts JsonWriter.write".to_owned(),
                    "src/lib.ts CsvWriter.write".to_owned(),
                ],
            ),
            // members that hold an arrow (a def named like a method) are targets too, as they are on a plain receiver
            (
                "arrows.write".to_owned(),
                vec![
                    "src/lib.ts ArrowA.write".to_owned(),
                    "src/lib.ts ArrowB.write".to_owned(),
                ],
            ),
            // `undefined` in the union has no property and adds nothing
            (
                "maybe?.write".to_owned(),
                vec![
                    "src/lib.ts JsonWriter.write".to_owned(),
                    "src/lib.ts CsvWriter.write".to_owned(),
                ],
            ),
            // an inherited method is where it is declared
            one("child.bump", "src/lib.ts Counter.bump"),
            // a method of an object literal is a def (D9)
            one("api.load", "src/lib.ts api.load"),
            // merged declarations: one target per declaration, told apart by their nth
            (
                "reader.read".to_owned(),
                vec![
                    "src/lib.ts Reader.read".to_owned(),
                    "src/lib.ts Reader.read#1".to_owned(),
                ],
            ),
        ];
        assert_eq!(found, expected);
    }

    #[test]
    fn sem_contract_a_function_typed_value_is_one_even_when_optional_or_in_a_union() {
        let sem = Sem::open(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolve"),
            Path::new("src/main.ts"),
        )
        .unwrap();
        let unknown = |site: &str| {
            (
                site.to_owned(),
                vec!["unknown function_typed_value".to_owned()],
            )
        };
        // `cb?.()` where `cb` may be undefined, and `result.mutate()` where `result` is a union whose every member holds a function (what a
        // react-query mutation looks like): both are calls through a function-typed value (D16), not a union problem and not "no declaration"
        assert_eq!(
            resolved_sites(&sem, "src/main.ts", "optional_callbacks"),
            [unknown("cb"), unknown("result.mutate")]
        );
    }

    #[test]
    fn sem_contract_function_typed_value_is_a_value_that_is_declared_as_one() {
        let sem = Sem::open(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolve"),
            Path::new("src/main.ts"),
        )
        .unwrap();
        let one = |site: &str, target: &str| (site.to_owned(), vec![target.to_owned()]);
        // a `let` that holds an arrow (only `const` names a def) and a destructured binding are value holders: D16
        assert_eq!(
            resolved_sites(&sem, "src/main.ts", "value_holders"),
            [
                one("later", "unknown function_typed_value"),
                one("onSave", "unknown function_typed_value")
            ]
        );
        // a callee that is not a name has no symbol, so it is not a value holder and nobody declared it: the engine has nothing to say, and the
        // count of `function_typed_value` does not swallow it
        assert_eq!(
            resolved_sites(&sem, "src/main.ts", "no_symbol"),
            [
                one("makeHandler", "src/lib.ts makeHandler"),
                one("makeHandler()", "unknown no_declaration"),
                one("(flag ? plain : second)", "unknown no_declaration"),
            ]
        );
    }

    #[test]
    fn sem_contract_an_external_that_is_declared_as_a_value_is_an_external() {
        let sem = Sem::open(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolve"),
            Path::new("src/main.ts"),
        )
        .unwrap();
        let one = |site: &str, target: &str| (site.to_owned(), vec![target.to_owned()]);
        // `export declare const useThing: () => number` and a property of function type in a package are named calls into the package, as a function
        // declaration would be; they are not D16's function-typed values, which are the ones declared in the project
        assert_eq!(
            resolved_sites(&sem, "src/main.ts", "external_value_holders"),
            [
                one("useThing", "external pkg useThing"),
                one("toolbox.run", "external pkg toolbox.run")
            ]
        );
    }

    #[test]
    fn sem_contract_this_dot_method_is_the_method_of_the_class() {
        let sem = Sem::open(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolve"),
            Path::new("src/main.ts"),
        )
        .unwrap();
        assert_eq!(
            resolved_sites(&sem, "src/lib.ts", "Greeter.run"),
            [(
                "this.helper".to_owned(),
                vec!["src/lib.ts Greeter.helper".to_owned()]
            )]
        );
    }

    #[test]
    fn describe_names_a_def_by_its_marker_or_by_where_it_is() {
        use analyzer_core::graph::StableDef;
        let resolve = Sem::open(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolve"),
            Path::new("src/main.ts"),
        )
        .unwrap();
        // a def with a marker is named by it: it survives a rename or a move
        let plain = resolve.entry("src/lib.ts", "plain", 0, STYLE).unwrap();
        let info = resolve.describe(plain, STYLE);
        assert_eq!(info.id, StableDef::Marked("PLAIN-0001".into()));
        assert_eq!(info.qname, "plain");
        assert_eq!(info.file, "src/lib.ts");
        assert_eq!(info.range, resolve.def(plain, STYLE).range);
        // one without is named by its file, qualname and which of the same-named defs it is
        let bump = resolve
            .entry("src/lib.ts", "Counter.bump", 0, STYLE)
            .unwrap();
        assert_eq!(
            resolve.describe(bump, STYLE).id,
            StableDef::Path {
                module: "src/lib.ts".into(),
                qname: "Counter.bump".into(),
                nth: 0
            }
        );
        // a getter and its setter have one qualname: the second is nth 1
        let entries = entries();
        let setter = entries.entry("src/main.ts", "Box.size", 1, STYLE).unwrap();
        assert_eq!(
            entries.describe(setter, STYLE).id,
            StableDef::Path {
                module: "src/main.ts".into(),
                qname: "Box.size".into(),
                nth: 1
            }
        );
        // and a class is described like a function
        let class = entries.entry("src/main.ts", "Box", 0, STYLE).unwrap();
        assert_eq!(
            entries.describe(class, STYLE).kind,
            analyzer_core::outline::DefKind::Class
        );
    }

    #[test]
    fn sem_contract_in_a_script_the_project_function_with_a_body_wins_over_the_libs_global() {
        let sem = Sem::open(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/script-global"),
            Path::new("main.ts"),
        )
        .unwrap();
        let one = |site: &str, target: &str| (site.to_owned(), vec![target.to_owned()]);
        // `function fetch() {}` in a script is the same symbol as the DOM's `fetch`; the checker lists both declarations, and a call to the project's
        // function is not a call into the browser. Our choice among the engine's declarations (ENGINE.md): one with a body in project code wins when the
        // others are in a bundled library or a .d.ts
        assert_eq!(
            resolved_sites(&sem, "main.ts", "caller"),
            [
                one("fetch", "main.ts fetch"),
                one("open", "main.ts open"),
                // the project's `declare function alert` has no body, so nothing is preferred: both targets stay, as the engine says
                (
                    "alert".to_owned(),
                    vec![
                        "main.ts alert".to_owned(),
                        "external typescript:lib alert".to_owned()
                    ]
                ),
            ]
        );
    }
}
