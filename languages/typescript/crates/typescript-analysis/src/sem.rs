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

use typescript_syntax::{Def, Dialect, MarkerStyle, Outline, Span};

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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "TS4 step 2b walks bodies from it")
    )]
    pub(crate) nodes: Vec<Option<P<Node>>>,
    /// The def a node is, for the nodes in `nodes`: the way back, and how the walk knows a nested node is a def of its own.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "TS4 step 2b stops the walk at a def with it")
    )]
    pub(crate) def_of: HashMap<P<Node>, usize>,
}

/// The files' defs made so far, by file and marker label.
type FileDefsCache = HashMap<(P<SourceFile>, String), Arc<FileDefs>>;

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

    /// The file at `relative` (to the root, with `/`; `./` and `..` are folded), if the program holds it.
    #[cfg_attr(
        not(any(test, feature = "__proof")),
        expect(dead_code, reason = "TS4 finds an entry with it")
    )]
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
        let key = (file.node, style.label.to_owned());
        if let Some(found) = self.file_defs.borrow().get(&key) {
            return Some(Arc::clone(found));
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
        self.file_defs.borrow_mut().insert(key, Arc::clone(&made));
        Some(made)
    }

    /// The `occurrence`th def (0-based, in source order: the number `StableDef::Path::nth` holds) with this `qualname` in the file at `file`
    /// (relative to the root). A qualname is not unique in a file (overloads, a getter and a setter), so the caller says which.
    #[cfg_attr(
        not(any(test, feature = "__proof")),
        expect(dead_code, reason = "TS4 step 4 starts a graph from it")
    )]
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "walk.rs turns these into the solver's sites")
    )]
    pub(crate) fn call_sites(&self, func: Func<'_>, style: MarkerStyle<'_>) -> Option<Vec<Span>> {
        let defs = self.file_defs(func.file, style)?;
        let node = defs.nodes[func.def]?;
        let _scope = self._region.enter();
        let mut collector = Collector {
            text: func.file.node.text(),
            def_of: &defs.def_of,
            own: node,
            sites: Vec::new(),
        };
        collector.body_of(node);
        collector.sites.sort();
        Some(collector.sites)
    }

    /// What the outline says the def is (qualname, kind, range, marker).
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "TS4 step 3's `describe` is built on it")
    )]
    pub(crate) fn def(&self, func: Func<'_>, style: MarkerStyle<'_>) -> Def {
        let defs = self
            .file_defs(func.file, style)
            .expect("a Func is made from an outline, so its file has one");
        defs.outline.defs[func.def].clone()
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
/// not; and a class's static block and property initializers are the class's own body. Only plain calls are sites so far (`f(x)`, `a.b(x)`, `a?.b(x)`,
/// `super(x)`): `new`, a tagged template, a decorator, a JSX element and an accessor are TS4 steps 5 and 6.
struct Collector<'a> {
    text: &'a str,
    def_of: &'a HashMap<P<Node>, usize>,
    /// The def being walked: it is in `def_of` too, and must not stop its own walk.
    own: P<Node>,
    sites: Vec<Span>,
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
        if node != self.own && self.def_of.contains_key(&node) {
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
            // cannot contain a callable node
            Kind::Identifier
            | Kind::ImportEqualsDeclaration
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
                self.collect(node.name());
                self.collect(node.initializer());
                return;
            }
            Kind::CallExpression => {
                if is_plain_call(node)
                    && let Some(callee) = node.expression()
                {
                    self.sites.push(own_span(self.text, callee));
                }
                // not the type arguments
                self.collect(node.expression());
                for &argument in node.arguments() {
                    self.collect(Some(argument));
                }
                return;
            }
            Kind::NewExpression => {
                self.collect(node.expression());
                for &argument in node.arguments() {
                    self.collect(Some(argument));
                }
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
        .take_while(|dir| dir.starts_with(root))
        .find_map(|dir| {
            ["tsconfig.json", "jsconfig.json"]
                .iter()
                .map(|name| dir.join(name))
                .find(|path| path.is_file())
        })
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
    tspath::normalize_slashes(&path.to_string_lossy())
}

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
        // `new`, a tagged template, `import()`, `require()` and a property read are not plain calls (steps 5 and 6, or never)
        assert_eq!(found.get("not_sites"), None);
        // a class: `extends`, a property initializer, a static property, a static block and a decorator are its body; its methods are not
        expect("Widget", &["mixin", "f", "g", "a.init", "dec"]);
        expect("Widget.method", &["f"]);
        expect("Widget.value", &["g"]);
        expect("Widget.constructor", &["super", "f"]);
        assert_eq!(found.get("Widget.decorated"), None);
        // an object-literal method is a def; a named function expression is a def; overloads have one body
        expect("table.load", &["f"]);
        expect("expression_names", &["[1, 2].map"]);
        expect("expression_names.<locals>.named", &["g"]);
        expect("overloaded", &["f"]);
        expect("arrow_def", &["f", "g"]);
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
            "Widget",
            "Widget.method",
            "Widget.value",
            "Widget.constructor",
            "table.load",
            "expression_names",
            "expression_names.<locals>.named",
            "overloaded",
            "arrow_def",
        ];
        let others: Vec<_> = found
            .keys()
            .filter(|q| !listed.contains(&q.as_str()))
            .collect();
        assert!(others.is_empty(), "{others:?}");
    }
}
