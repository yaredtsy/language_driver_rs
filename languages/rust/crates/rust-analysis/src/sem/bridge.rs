//! The bridge between the engine's defs and the outline's (RS4 §5, D20). Every def that becomes a `Func` goes through here, so the graph names a def
//! the way the outline does: the engine says *which* def a call reaches, `origin.rs` says *where it is written*, and the outline of that file says
//! *what it is called*. All of it is inside one `attach_db` per request (`attached`), and no engine type leaves this module.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::rc::Rc;

use analyzer_core::edit::MarkerStyle;
use analyzer_core::graph::{DefInfo, StableDef};
use analyzer_core::outline::{DefKind, Outline};
use analyzer_core::text::Span;
use ra_ap_hir::{InMacroFile, Macro, MacroCallId, Semantics, Variant, attach_db};
use ra_ap_ide_db::RootDatabase;
use ra_ap_syntax::{AstNode, NodeOrToken, SyntaxNode, SyntaxToken, TextRange, ast};
use rust_syntax::Edition;

use super::load::Loaded;
use super::origin::{Cause, Item, Owner};
use super::sites::Sites;

/// A def of one file's outline (`rust_syntax::outline`, the rule RS1 froze) and the engine's def at the same place. It is `Copy`, `Eq` and `Hash` for
/// core's solver, and is valid for the `Sem`'s life and no longer (`'p`): the engine's ids mean something only while the database lives.
///
/// Only `Sem::entry` and `Sem::bridge` make one, so a `Func` always has a row in the outline (`describe` cannot miss).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Func<'p> {
    /// The file, as `Sem::open` numbers them: in the order a request first touches them.
    file: u32,
    /// The index of the def in the file's outline.
    def: usize,
    item: Item,
    request: PhantomData<&'p ()>,
}

impl Func<'_> {
    pub(crate) fn item(self) -> Item {
        self.item
    }

    /// The file's number (`Sem::open`).
    pub(super) fn file(self) -> u32 {
        self.file
    }
}

/// A def found by qualname (`Sem::defs_named`), as owned data.
pub(crate) struct DefFound {
    pub(crate) occurrence: u16,
    pub(crate) kind: DefKind,
    pub(crate) range: Span,
    pub(crate) marker: Option<String>,
}

/// What an engine def is, in the graph's terms.
pub(crate) enum Bridged<'p> {
    /// A def of the project: a node, with the id the outline gives it.
    Func(Func<'p>),
    /// Not entered (`Owner::External`, and its cause): named, never walked.
    External {
        package: String,
        name: String,
        cause: Cause,
    },
    /// Project code the outline of its file has no def for: the two parses disagree (or the def has no name). A bug that must show on the graph,
    /// not hide: the caller makes it `Unresolved(def_not_in_outline)`.
    NotInOutline,
}

/// One file's outline, as the request uses it.
struct FileDefs {
    /// Relative to the root, with `/`.
    path: String,
    outline: Outline,
    /// For each def, how many defs before it have its qualname (D18).
    occurrences: Vec<u16>,
    /// For each def: it has a marker and no def before it in the file has the same one (D34).
    owns_marker: Vec<bool>,
}

/// The files a request has asked about. A file is known by its path relative to the root, and a `Func` holds its number: the engine's own handle for a
/// file is looked up when the file is opened and not kept (it is one more thing that means something only inside the database).
#[derive(Default)]
struct Files {
    /// Only looked up, never iterated (rule 9).
    numbered: HashMap<String, u32>,
    opened: Vec<Opened>,
}

struct Opened {
    defs: Rc<FileDefs>,
    /// The engine's syntax tree of the file, which `Semantics::to_def` is asked about. The same text `defs` was made from.
    tree: ast::SourceFile,
}

/// Everything one request asks of the engine. Built inside `attach_db` by `attached`, dropped inside it, never kept: a `Semantics` made or used
/// outside it is a bug (S-RS2).
pub(crate) struct Sem<'p> {
    loaded: &'p Loaded,
    sema: Semantics<'p, RootDatabase>,
    style: MarkerStyle<'p>,
    files: RefCell<Files>,
    /// `sites` of each function asked about, made once per request.
    sites: RefCell<HashMap<Func<'p>, Rc<Sites>>>,
}

/// The text of the project file at `path` as the engine loaded it, for the wire (`Project: Sources`). One `attach_db` for the call, as every use of the engine
/// is; the style is not read.
pub(crate) fn text_of(loaded: &Loaded, path: &str) -> Option<String> {
    attached(loaded, MarkerStyle::docstring(""), |sem| sem.text(path))
}

/// One `attach_db` for the whole request, with the `Sem` built inside it (§2): every engine call and the solver run inside `f`. `style` is how the
/// request reads markers (`Options::marker`, RS4 step 4).
pub(crate) fn attached<'p, R>(
    loaded: &'p Loaded,
    style: MarkerStyle<'p>,
    f: impl FnOnce(&Sem<'p>) -> R,
) -> R {
    attach_db(loaded.db(), || f(&Sem::new(loaded, style)))
}

impl<'p> Sem<'p> {
    fn new(loaded: &'p Loaded, style: MarkerStyle<'p>) -> Sem<'p> {
        Sem {
            loaded,
            sema: Semantics::new(loaded.db()),
            style,
            files: RefCell::new(Files::default()),
            sites: RefCell::new(HashMap::new()),
        }
    }

    /// The function of the graph for the def written in `file` (relative to the root) that the outline names `(qualname, occurrence)`. `None` for a file
    /// that is not in `Project::files()`, for no such def, for a kind that cannot be an entry (a trait, a module, a type alias, a macro, an impl), and
    /// for a def in code a `cfg` turns off: the engine has no def there.
    pub(crate) fn entry(&self, file: &str, qualname: &str, occurrence: u16) -> Option<Func<'p>> {
        let file = self.open(file)?;
        let (defs, tree) = self.opened(file);
        let at = (0..defs.outline.defs.len()).find(|&at| {
            defs.outline.defs[at].qualname == qualname && defs.occurrences[at] == occurrence
        })?;
        let def = &defs.outline.defs[at];
        if !can_be_entry(&def.kind) {
            return None;
        }
        let item = self.item_named(&tree, def.name_range, &def.kind)?;
        Some(Func {
            file,
            def: at,
            item,
            request: PhantomData,
        })
    }

    /// The defs of `file` that the outline names `qualname`, in source order: which of them is which (`occurrence`), what each is, where, and its marker
    /// (D15: a qualname is not unique in a file). `None` for a file that is not in `Project::files()` (D33's rule: project code only). Every def the
    /// outline lists is offered, whether or not it can be an entry: a person chooses among them, and `call_graph` says which cannot start a graph.
    pub(crate) fn defs_named(&self, file: &str, qualname: &str) -> Option<Vec<DefFound>> {
        let file = self.open(file)?;
        let (defs, _) = self.opened(file);
        Some(
            defs.outline
                .defs
                .iter()
                .zip(&defs.occurrences)
                .filter(|(def, _)| def.qualname == qualname)
                .map(|(def, &occurrence)| DefFound {
                    occurrence,
                    kind: def.kind.clone(),
                    range: def.range,
                    marker: def.marker.clone(),
                })
                .collect(),
        )
    }

    /// What the engine's def is in the graph: its outline def when it is project code, else where it comes from. A variant is passed as the enum
    /// that owns it: `bridge_variant`, which also names an external one with its variant.
    pub(crate) fn bridge(&self, item: Item) -> Bridged<'p> {
        self.bridged(self.loaded.owner_of(&self.sema, item), item)
    }

    /// `bridge` for a variant: a project variant is its enum (`Func` of the enum), an external one is named with its variant (`owner_of_variant`).
    pub(crate) fn bridge_variant(&self, variant: Variant) -> Bridged<'p> {
        let item = Item::of_variant(self.loaded.db(), variant);
        self.bridged(self.loaded.owner_of_variant(&self.sema, variant), item)
    }

    fn bridged(&self, owner: Owner, item: Item) -> Bridged<'p> {
        match owner {
            Owner::External {
                package,
                name,
                cause,
            } => Bridged::External {
                package,
                name,
                cause,
            },
            Owner::Project { file, name } => {
                let found = name.and_then(|name| {
                    let file = self.open(&file)?;
                    let (defs, _) = self.opened(file);
                    let at = defs
                        .outline
                        .defs
                        .iter()
                        .position(|d| d.name_range == name)?;
                    Some((file, at))
                });
                match found {
                    Some((file, def)) => Bridged::Func(Func {
                        file,
                        def,
                        item,
                        request: PhantomData,
                    }),
                    None => Bridged::NotInOutline,
                }
            }
        }
    }

    /// What the def is, in the core's terms: `Marked` by its marker when it owns one, else named by where it is (the file relative to the root, its
    /// qualname, and which of the same-named defs of the file it is, in source order). The qname is the qualname.
    pub(crate) fn describe(&self, func: Func<'p>) -> DefInfo {
        let (defs, _) = self.opened(func.file);
        let def = &defs.outline.defs[func.def];
        let occurrence = defs.occurrences[func.def];
        // a marker another def of the file owns is not this def's id (D34): it is named by where it is, as a def with no marker is
        let id = match &def.marker {
            Some(marker) if defs.owns_marker[func.def] => StableDef::Marked(marker.clone()),
            _ => StableDef::Path {
                module: defs.path.clone(),
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
            defs.path.clone(),
            def.range,
        )
    }

    /// Whether the def has a marker that another def of its file owns, so `describe` names it by where it is (D34): the caller counts it
    /// `duplicate_marker`.
    pub(crate) fn has_a_marker_it_does_not_own(&self, func: Func<'p>) -> bool {
        let (defs, _) = self.opened(func.file);
        defs.outline.defs[func.def].marker.is_some() && !defs.owns_marker[func.def]
    }

    /// The number of the project file at `path` (relative to the root, with `/`), the file opened on first use: its text is the engine's, not the
    /// disk's, so its offsets are the ones `Owner` reports and the two outlines agree on every byte. `None` for a path that is no file of the load
    /// under the root, and for a file no crate has a module for: `Project::files()`'s own test (RS3 §2), for this one file.
    fn open(&self, path: &str) -> Option<u32> {
        if let Some(&known) = self.files.borrow().numbered.get(path) {
            return Some(known);
        }
        let (tree, edition) = self.find(path)?;
        let text = tree.syntax().text().to_string();
        let outline = rust_syntax::outline(&text, edition_of(edition), self.style);
        let defs = Rc::new(FileDefs {
            path: path.to_owned(),
            occurrences: rust_syntax::occurrences(&outline),
            owns_marker: marker_owners(&outline),
            outline,
        });
        let mut files = self.files.borrow_mut();
        let number = u32::try_from(files.opened.len()).expect("fewer than 2^32 files");
        files.numbered.insert(path.to_owned(), number);
        files.opened.push(Opened { defs, tree });
        Some(number)
    }

    /// The engine's syntax tree of the project file at `path`, and its edition: `None` for a path that is no file of the load under the root, and for a
    /// file no crate has a module for (`Project::files()`'s own test, RS3 §2).
    fn find(&self, path: &str) -> Option<(ast::SourceFile, ra_ap_syntax::Edition)> {
        let db = self.loaded.db();
        self.loaded.vfs().iter().find_map(|(file, vfs_path)| {
            if vfs_path.as_path()?.extension()? != "rs"
                || self.loaded.root_relative(vfs_path)?.as_str() != path
            {
                return None;
            }
            let editioned = self.sema.attach_first_edition_opt(file)?;
            Some((self.sema.parse(editioned), editioned.edition(db)))
        })
    }

    /// The text of the project file at `path` **as the engine holds it**, the text every offset of the graph counts in (a byte-order mark, a `\r\n`
    /// included): the wire turns a site into a line and a column with it.
    pub(crate) fn text(&self, path: &str) -> Option<String> {
        let (tree, _) = self.find(path)?;
        Some(tree.syntax().text().to_string())
    }

    /// An opened file's outline and syntax tree.
    fn opened(&self, file: u32) -> (Rc<FileDefs>, ast::SourceFile) {
        let files = self.files.borrow();
        let opened = &files.opened[file as usize];
        (Rc::clone(&opened.defs), opened.tree.clone())
    }

    /// The engine's def whose name is at `name` in the file, of the kind the outline says (`Semantics::to_def`). `None` where the engine has no def: in
    /// code a `cfg` turns off.
    fn item_named(&self, tree: &ast::SourceFile, name: Span, kind: &DefKind) -> Option<Item> {
        let range = text_range(name);
        let node = node_at(tree, range)?;
        match kind {
            DefKind::Function | DefKind::Method => self
                .sema
                .to_def(&named::<ast::Fn>(&node, range)?)
                .map(Item::Function),
            DefKind::Struct | DefKind::Enum => self
                .sema
                .to_def(&named::<ast::Adt>(&node, range)?)
                .map(Item::Adt),
            DefKind::Other(what) if what == "const" => self
                .sema
                .to_def(&named::<ast::Const>(&node, range)?)
                .map(Item::Const),
            DefKind::Other(what) if what == "static" => self
                .sema
                .to_def(&named::<ast::Static>(&node, range)?)
                .map(Item::Static),
            _ => None,
        }
    }

    pub(super) fn sema(&self) -> &Semantics<'p, RootDatabase> {
        &self.sema
    }

    pub(super) fn loaded(&self) -> &'p Loaded {
        self.loaded
    }

    /// The file (relative to the root) and the name's range of a `Func`, in the engine's bytes: the oracle test compares a target by it.
    #[cfg(test)]
    pub(super) fn name_of(&self, func: Func<'p>) -> (String, Span) {
        let (defs, _) = self.opened(func.file);
        (defs.path.clone(), defs.outline.defs[func.def].name_range)
    }

    /// The engine's function whose name is at `name` in the project file `file`, for the oracle test, which meets targets as the engine names them.
    #[cfg(test)]
    pub(super) fn function_at(&self, file: &str, name: Span) -> Option<ra_ap_hir::Function> {
        let file = self.open(file)?;
        let (_, tree) = self.opened(file);
        match self.item_named(&tree, name, &DefKind::Function)? {
            Item::Function(function) => Some(function),
            _ => None,
        }
    }

    // What `sites.rs` asks of the engine. They are here because they use what the `Sem` keeps, and `Sem` keeps it private.

    pub(super) fn sites_cache(&self) -> &RefCell<HashMap<Func<'p>, Rc<Sites>>> {
        &self.sites
    }

    /// The whole def, in the engine's bytes of its file.
    pub(super) fn def_range(&self, func: Func<'p>) -> Span {
        let (defs, _) = self.opened(func.file);
        defs.outline.defs[func.def].range
    }

    /// The def's body: a function's block, a const's or static's initializer. `None` for a def with none (a trait method without a default, a foreign
    /// function) and for a struct or enum.
    pub(super) fn body_of(&self, func: Func<'p>) -> Option<SyntaxNode> {
        let (defs, tree) = self.opened(func.file);
        let range = text_range(defs.outline.defs[func.def].name_range);
        let node = node_at(&tree, range)?;
        match func.item() {
            Item::Function(_) => named::<ast::Fn>(&node, range)?
                .body()
                .map(|body| body.syntax().clone()),
            Item::Const(_) => named::<ast::Const>(&node, range)?
                .body()
                .map(|body| body.syntax().clone()),
            Item::Static(_) => named::<ast::Static>(&node, range)?
                .body()
                .map(|body| body.syntax().clone()),
            Item::Adt(_) => None,
        }
    }

    /// The expansion of a macro call and which expansion it is, `None` when the engine cannot expand it.
    pub(super) fn expand(
        &self,
        call: &ast::MacroCall,
    ) -> Option<(SyntaxNode, Option<MacroCallId>)> {
        self.sema
            .expand_macro_call(call)
            .map(|expanded| (expanded.value, expanded.file_id.macro_file()))
    }

    /// Where a token of the expansion `file` came from **in the arguments of the macro call that made it**, one expansion out: the range in the text of the
    /// file the call is written in. `None` for a token the macro's own body made (its span is not among the call's arguments). Only for a call written
    /// inside another expansion: for one written in a file the engine maps a body token to the macro's definition, which is no answer to this question.
    pub(super) fn upmap_once(&self, file: MacroCallId, token: &SyntaxToken) -> Option<TextRange> {
        let up = InMacroFile {
            file_id: file,
            value: token.clone(),
        }
        .upmap_once(self.loaded.db());
        up.value.first().copied()
    }

    /// Where a node of an expansion is written in `file`, when it is written there (an argument of the macro call) and not made by the macro's body. The
    /// engine's range names a file of its own, and a range in another file (a project macro defined in another module) is no place in this body: it is
    /// compared with the def's file, never taken by its offsets alone.
    pub(super) fn original_range(&self, node: &SyntaxNode, file: u32) -> Option<Span> {
        let found = self.sema.original_range_opt(node)?;
        let db = self.loaded.db();
        let path = self
            .loaded
            .root_relative(self.loaded.vfs().file_path(found.file_id.file_id(db)))?;
        let (defs, _) = self.opened(file);
        if path != defs.path {
            return None;
        }
        Some(Span::new(
            found.range.start().into(),
            found.range.end().into(),
        ))
    }

    pub(super) fn macro_of(&self, call: &ast::MacroCall) -> Option<Macro> {
        self.sema.resolve_macro_call(call)
    }

    pub(super) fn is_project_macro(&self, mac: Macro) -> bool {
        self.loaded.is_project_macro(&self.sema, mac)
    }

    /// Whether the engine has an expression for this one in a body it lowered: it has none for what a `cfg` turns off, inside an active body. A lowered
    /// expression has a type, even an unknown one.
    pub(super) fn lowered(&self, expr: &ast::Expr) -> bool {
        self.sema.type_of_expr(expr).is_some()
    }
}

fn text_range(span: Span) -> TextRange {
    TextRange::new(span.start.into(), span.end.into())
}

/// The node that holds the bytes at `range`.
fn node_at(tree: &ast::SourceFile, range: TextRange) -> Option<SyntaxNode> {
    match tree.syntax().covering_element(range) {
        NodeOrToken::Node(node) => Some(node),
        NodeOrToken::Token(token) => token.parent(),
    }
}

/// The nearest `N` around `node` whose name is exactly at `range`.
fn named<N: ast::HasName>(node: &SyntaxNode, range: TextRange) -> Option<N> {
    node.ancestors().find_map(N::cast).filter(|found| {
        found
            .name()
            .is_some_and(|name| name.syntax().text_range() == range)
    })
}

/// Which kinds of def start a graph (§1): a function, a method, a struct, a union, an enum, a const, a static. A trait, a module, a type alias, a macro
/// and an impl do not.
fn can_be_entry(kind: &DefKind) -> bool {
    match kind {
        DefKind::Function | DefKind::Method | DefKind::Struct | DefKind::Enum => true,
        DefKind::Other(what) => what == "const" || what == "static",
        _ => false,
    }
}

/// Which defs own their marker (`FileDefs::owns_marker`): the first in source order (by where the def starts, not by the order of the outline) of each
/// text, as TypeScript's.
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

/// The engine's edition, as `rust-syntax` spells it.
fn edition_of(edition: ra_ap_syntax::Edition) -> Edition {
    match edition {
        ra_ap_syntax::Edition::Edition2015 => Edition::E2015,
        ra_ap_syntax::Edition::Edition2018 => Edition::E2018,
        ra_ap_syntax::Edition::Edition2021 => Edition::E2021,
        // `Edition` is `#[non_exhaustive]` on neither side; a newer one the pinned engine does not have is a bump's to add
        ra_ap_syntax::Edition::Edition2024 => Edition::E2024,
    }
}

#[cfg(test)]
mod tests {
    use analyzer_core::outline::DefKind;
    use ra_ap_hir::Adt;

    use super::*;
    use crate::sem::testing::{STYLE, disk_outline, policy};

    /// RS4 §8, "the join": every def the outline lists in the fixture is found by `entry`, `describe` of it gives the same (file, qualname, occurrence,
    /// kind, range) back, and the way back (`bridge` of the engine's def) is the same `Func`. The exceptions are stated, each with its reason.
    #[test]
    fn every_def_the_outline_lists_is_found_and_named_the_same() {
        let (loaded, root) = policy("join_policy");
        let files = loaded.files();
        assert_eq!(
            files,
            [
                "src/bom.rs",
                "src/calls.rs",
                "src/cfg_off.rs",
                "src/constructions.rs",
                "src/dispatch.rs",
                "src/external.rs",
                "src/lib.rs",
                "src/macros.rs",
                "src/nested/deep.rs",
                "src/nested/mod.rs",
                "src/passed.rs",
                "src/shapes.rs",
                "src/shared.rs"
            ],
            "src/stray.rs is on disk and in no module's tree"
        );
        let mut joined = 0;
        // (file, qualname) of the defs that are not entries, and of those the engine has no def for
        let mut not_entries: Vec<(String, String)> = Vec::new();
        let mut cfg_off: Vec<(String, String)> = Vec::new();
        attached(&loaded, STYLE, |sem| {
            for file in &files {
                let outline = disk_outline(&root, file);
                let occurrences = rust_syntax::occurrences(&outline);
                for (def, occurrence) in outline.defs.iter().zip(occurrences) {
                    let found = sem.entry(file, &def.qualname, occurrence);
                    let name = (file.clone(), def.qualname.clone());
                    // exception 1: kinds that cannot be an entry (§1)
                    if !can_be_entry(&def.kind) {
                        assert!(found.is_none(), "{name:?} is no entry");
                        assert!(
                            matches!(
                                def.kind,
                                DefKind::Trait
                                    | DefKind::Module
                                    | DefKind::Type
                                    | DefKind::Other(_)
                            ),
                            "{name:?}: {:?}",
                            def.kind
                        );
                        not_entries.push(name);
                        continue;
                    }
                    // exception 2: code a `cfg` turns off, where the engine has no def
                    let Some(func) = found else {
                        cfg_off.push(name);
                        continue;
                    };
                    let info = sem.describe(func);
                    assert_eq!(
                        (
                            info.file.as_str(),
                            info.qualname.as_str(),
                            info.occurrence,
                            &info.kind,
                            info.range
                        ),
                        (
                            file.as_str(),
                            def.qualname.as_str(),
                            occurrence,
                            &def.kind,
                            def.range
                        ),
                        "describe of the entry {name:?}"
                    );
                    // and back: the engine's def, through its owner and the outline, is the same function
                    match sem.bridge(func.item()) {
                        Bridged::Func(back) => assert!(back == func, "the way back of {name:?}"),
                        Bridged::External { .. } => panic!("{name:?} came back External"),
                        Bridged::NotInOutline => panic!("{name:?} came back NotInOutline"),
                    }
                    joined += 1;
                }
            }
        });
        assert_eq!(
            cfg_off,
            [("src/lib.rs".to_owned(), "disabled".to_owned())],
            "the one #[cfg(any())] def is the listed exception, and no other"
        );
        // the traits, the modules, the type alias and the macro of the fixture
        let names: Vec<_> = not_entries
            .iter()
            .map(|(file, qualname)| format!("{file}:{qualname}"))
            .collect();
        assert_eq!(
            names,
            [
                "src/calls.rs:Declared",
                "src/constructions.rs:make_tuple!",
                "src/dispatch.rs:Step",
                "src/lib.rs:Writer",
                "src/lib.rs:Make",
                "src/lib.rs:tests",
                "src/lib.rs:make_fn!",
                "src/macros.rs:log!",
                "src/macros.rs:both!",
                "src/macros.rs:nested!",
                "src/macros.rs:statements!",
                "src/macros.rs:wrap!",
                "src/macros.rs:wrap_argument!",
                "src/macros.rs:nest_wrap!",
                "src/macros.rs:make_function!",
                "src/macros.rs:lost_inside!",
                "src/passed.rs:Build",
                "src/passed.rs:make_function!",
                "src/shapes.rs:Alias",
                "src/shared.rs:from_elsewhere!",
            ],
            "the five traits, the inline module, the thirteen macros, the type alias"
        );
        // every other def: 18 in lib.rs, 8 in constructions.rs, 16 in shapes.rs, 29 in calls.rs, 30 in dispatch.rs, 21 in macros.rs, 11 in cfg_off.rs, 19 in passed.rs, 2 in external.rs, 1 in shared.rs, 1 in nested/mod.rs, 3 in deep.rs, 3 after a
        // byte-order mark
        assert_eq!(joined, 162);
    }

    /// The join (RS4 §8) over the context workspace, which has no `#[cfg]`-disabled def and no exception but the kinds that cannot be an entry: every def the
    /// outline lists is found by `entry`, `describe` of it gives the same (file, qualname, occurrence, kind, range) back, and the way back is the same `Func`.
    #[test]
    fn every_def_of_the_context_workspace_is_found_and_named_the_same() {
        let (loaded, root) = crate::sem::testing::workspace("context", "join_context");
        let mut joined = 0;
        attached(&loaded, STYLE, |sem| {
            for file in loaded.files() {
                let outline = disk_outline(&root, &file);
                for (def, occurrence) in outline.defs.iter().zip(rust_syntax::occurrences(&outline))
                {
                    let found = sem.entry(&file, &def.qualname, occurrence);
                    if !can_be_entry(&def.kind) {
                        assert!(found.is_none(), "{file}:{} is no entry", def.qualname);
                        continue;
                    }
                    let func =
                        found.unwrap_or_else(|| panic!("{file}:{} is an entry", def.qualname));
                    let info = sem.describe(func);
                    assert_eq!(
                        (
                            info.file.as_str(),
                            info.qualname.as_str(),
                            info.occurrence,
                            &info.kind,
                            info.range
                        ),
                        (
                            file.as_str(),
                            def.qualname.as_str(),
                            occurrence,
                            &def.kind,
                            def.range
                        )
                    );
                    match sem.bridge(func.item()) {
                        Bridged::Func(back) => assert!(back == func, "{file}:{}", def.qualname),
                        _ => panic!("{file}:{} did not come back a Func", def.qualname),
                    }
                    joined += 1;
                }
            }
        });
        // 5 in lib.rs (the trait's `write`, `Json`, `Csv` and the two impls' `write`), 4 in generic.rs, 4 in callback.rs, 1 in dyn_local.rs, 2 in dyn_param.rs, 4 in
        // factory.rs, 4 in holder.rs and 2 in recursive.rs
        assert_eq!(joined, 26);
    }

    /// What `entry` answers `None` to, besides a def a `cfg` turns off (the join test): no file, a file no module reaches, a kind that cannot start a
    /// graph, a def that is not there.
    #[test]
    fn an_entry_that_is_not_one_is_none() {
        let (loaded, _root) = policy("entry_none");
        attached(&loaded, STYLE, |sem| {
            let none = |file: &str, qualname: &str, occurrence: u16| {
                assert!(
                    sem.entry(file, qualname, occurrence).is_none(),
                    "{file}:{qualname}:{occurrence}"
                );
            };
            // a file on disk that no `mod` reaches: it is not in `files()`
            none("src/stray.rs", "stray", 0);
            none("src/missing.rs", "plain", 0);
            // a path is relative to the root, spelled with `/`
            none("./src/lib.rs", "plain", 0);
            none("src\\lib.rs", "plain", 0);
            // kinds that cannot be an entry: a trait, a module, a macro, a type alias
            none("src/lib.rs", "Writer", 0);
            none("src/lib.rs", "tests", 0);
            none("src/lib.rs", "make_fn!", 0);
            none("src/shapes.rs", "Alias", 0);
            // `#[cfg(any())]`: the engine has no def
            none("src/lib.rs", "disabled", 0);
            // the occurrence counts defs of the name: one `plain`, two `Rec::get`
            none("src/lib.rs", "plain", 1);
            none("src/shapes.rs", "Rec::get", 2);
            none("src/lib.rs", "Plain", 0);
            // and what is found, found
            assert!(sem.entry("src/lib.rs", "plain", 0).is_some());
            assert!(sem.entry("src/shapes.rs", "Rec::get", 1).is_some());
            // a def after the byte-order mark: the engine's offsets and the outline's are the same bytes
            assert!(sem.entry("src/bom.rs", "Bommed::method", 0).is_some());
            // a def written inside a body, and one a derive or an `extern` block gives
            assert!(
                sem.entry("src/lib.rs", "with_inner::{fn}::inner", 0)
                    .is_some()
            );
            assert!(sem.entry("src/shapes.rs", "foreign", 0).is_some());
            assert!(sem.entry("src/lib.rs", "tests::a_test", 0).is_some());
        });
    }

    /// D34, as TypeScript's `describe`: a marker names the def that owns it; a later def of the same file with the same marker is named by where it is.
    #[test]
    fn describe_names_a_def_by_its_marker_or_by_where_it_is() {
        let (loaded, _root) = policy("describe");
        attached(&loaded, STYLE, |sem| {
            let id = |file: &str, qualname: &str, occurrence: u16| {
                let func = sem.entry(file, qualname, occurrence).unwrap();
                (
                    sem.describe(func).id,
                    sem.has_a_marker_it_does_not_own(func),
                )
            };
            let path = |file: &str, qname: &str, nth: u16| StableDef::Path {
                module: file.to_owned(),
                qname: qname.to_owned(),
                nth,
            };
            assert_eq!(
                id("src/lib.rs", "marked", 0),
                (StableDef::Marked("marked-one".to_owned()), false)
            );
            // two defs, one marker: the first owns it
            assert_eq!(
                id("src/lib.rs", "twin_a", 0),
                (StableDef::Marked("twin".to_owned()), false)
            );
            // the second is named by where it is, and the caller is told to count it `duplicate_marker`
            assert_eq!(
                id("src/lib.rs", "twin_b", 0),
                (path("src/lib.rs", "twin_b", 0), true)
            );
            // no marker: where it is, and not a duplicate
            assert_eq!(
                id("src/lib.rs", "plain", 0),
                (path("src/lib.rs", "plain", 0), false)
            );
            // two defs of one qualname are told apart by the occurrence, in source order
            assert_eq!(
                id("src/shapes.rs", "Rec::get", 0),
                (path("src/shapes.rs", "Rec::get", 0), false)
            );
            assert_eq!(
                id("src/shapes.rs", "Rec::get", 1),
                (path("src/shapes.rs", "Rec::get", 1), false)
            );
            // a file in a folder
            assert_eq!(
                id("src/nested/deep.rs", "Deep::new", 0),
                (path("src/nested/deep.rs", "Deep::new", 0), false)
            );
        });
    }

    /// `origin.rs`'s owner for the kinds RS4 added to it: a const, a static, an ADT, an associated const, a foreign function, and a variant, which is
    /// owned by its enum. A project def's owner holds where its **name** is, which is the outline's `name_range`.
    #[test]
    fn the_owner_of_every_kind_of_def_holds_the_file_and_where_its_name_is() {
        let (loaded, root) = policy("owner_kinds");
        attached(&loaded, STYLE, |sem| {
            let shapes = disk_outline(&root, "src/shapes.rs");
            let name_of = |qualname: &str| {
                shapes
                    .defs
                    .iter()
                    .find(|d| d.qualname == qualname)
                    .unwrap_or_else(|| panic!("{qualname}"))
                    .name_range
            };
            let owner_of = |qualname: &str| {
                let func = sem.entry("src/shapes.rs", qualname, 0).unwrap();
                (func.item(), loaded.owner_of(&sem.sema, func.item()))
            };
            let project = |qualname: &str| Owner::Project {
                file: "src/shapes.rs".to_owned(),
                name: Some(name_of(qualname)),
            };
            for qualname in [
                "LIMIT", "NAME", "Pair", "Rec", "Bits", "E", "Rec::K", "Rec::mk", "foreign",
                "limit",
            ] {
                assert_eq!(owner_of(qualname).1, project(qualname), "{qualname}");
            }

            // a variant is owned by its enum (and a struct or union by itself)
            let (item, enum_owner) = owner_of("E");
            let Item::Adt(adt @ Adt::Enum(e)) = item else {
                panic!("E is an enum");
            };
            let variants = e.variants(loaded.db());
            assert_eq!(variants.len(), 3);
            for variant in variants {
                let variant = ra_ap_hir::Variant::EnumVariant(variant);
                assert!(Item::of_variant(loaded.db(), variant) == Item::Adt(adt));
                assert_eq!(loaded.owner_of_variant(&sem.sema, variant), enum_owner);
            }
            let (pair, pair_owner) = owner_of("Pair");
            let Item::Adt(Adt::Struct(pair)) = pair else {
                panic!("Pair is a struct");
            };
            assert_eq!(
                loaded.owner_of_variant(&sem.sema, ra_ap_hir::Variant::Struct(pair)),
                pair_owner
            );
        });
    }

    /// Every call written in `file` that resolves to a function, as (the text of the callee, the engine's def), in source order: the part of RS4's site
    /// walk this step needs, as `origin.rs`'s tests have it.
    fn calls_in(sem: &Sem<'_>, file: &str) -> Vec<(String, Item)> {
        use ra_ap_hir::{CallableKind, ModuleDef, PathResolution};
        let (_, tree) = sem.opened(sem.open(file).unwrap());
        let mut out = Vec::new();
        for node in tree.syntax().descendants() {
            let function = if let Some(call) = ast::CallExpr::cast(node.clone()) {
                let callee = call.expr().unwrap();
                // path first, the callable type for what has no path or whose path names a value (RS4 §4)
                let by_path = match &callee {
                    ast::Expr::PathExpr(path) => {
                        path.path().and_then(|p| sem.sema.resolve_path(&p))
                    }
                    _ => None,
                };
                match by_path {
                    Some(PathResolution::Def(ModuleDef::Function(f))) => {
                        Some((callee.syntax().text().to_string(), f))
                    }
                    _ => match sem.sema.resolve_expr_as_callable(&callee).map(|c| c.kind()) {
                        Some(CallableKind::Function(f)) => {
                            Some((callee.syntax().text().to_string(), f))
                        }
                        _ => None,
                    },
                }
            } else if let Some(call) = ast::MethodCallExpr::cast(node) {
                sem.sema
                    .resolve_method_call_fallback(&call)
                    .and_then(|(function, _)| function.left())
                    .map(|f| (call.name_ref().unwrap().text().to_string(), f))
            } else {
                None
            };
            if let Some((text, f)) = function {
                out.push((text, Item::Function(f)));
            }
        }
        out
    }

    /// A def the bridge does not enter says why: `External`, named by its crate, with the cause (RS4 §5). A project def a macro made is in no file
    /// (`MacroFile`); a built-in derive's method has the trait's declaration, in `core`, as its source (`OtherCrate`); `std`'s own are `Dependency`, named
    /// by the defining crate, from its root, without the crate's name.
    #[test]
    fn a_def_that_is_not_project_code_comes_back_external_with_its_cause() {
        let (loaded, _root) = policy("bridge_external");
        attached(&loaded, STYLE, |sem| {
            let external = |item| match sem.bridge(item) {
                Bridged::External {
                    package,
                    name,
                    cause,
                } => (package, name, cause),
                Bridged::Func(func) => panic!("a Func: {:?}", sem.describe(func).qualname),
                Bridged::NotInOutline => panic!("NotInOutline"),
            };
            let lib: Vec<_> = calls_in(sem, "src/lib.rs")
                .into_iter()
                .map(|(text, item)| (text, matches!(sem.bridge(item), Bridged::Func(_))))
                .collect();
            // `helper()` in `plain`, the call in `twice`, `with_inner`'s `inner()` and the two twins are project code; `generated()` is not. The second `helper` is the
            // call inside `disabled`, which is `#[cfg(any())]`: name resolution answers for it (path first), although the engine has no def of that body.
            // A walk from a def's body never reaches it: this helper reads the whole file's text.
            assert_eq!(
                lib.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
                [
                    "write",
                    "write",
                    "helper",
                    "inner",
                    "twin_a",
                    "twin_b",
                    "helper",
                    "generated"
                ]
            );
            assert_eq!(
                lib.iter().map(|(_, project)| *project).collect::<Vec<_>>(),
                [true, true, true, true, true, true, true, false]
            );
            let generated = &calls_in(sem, "src/lib.rs")[7];
            assert_eq!(
                external(generated.1),
                (
                    "policy".to_owned(),
                    "generated".to_owned(),
                    Cause::MacroFile
                )
            );

            let shapes = calls_in(sem, "src/shapes.rs");
            let texts: Vec<_> = shapes.iter().map(|(t, _)| t.as_str()).collect();
            assert_eq!(texts, ["limit", "Derived::default", "clone"]);
            // a built-in derive's methods: written nowhere, and the trait's declaration in `core` is their source. Named by the type's crate, from its root
            for (at, name) in [
                (1, "shapes::<Derived as Default>::default"),
                (2, "shapes::<Derived as Clone>::clone"),
            ] {
                assert_eq!(
                    external(shapes[at].1),
                    ("policy".to_owned(), name.to_owned(), Cause::OtherCrate)
                );
            }
        });
    }
}
