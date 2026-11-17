//! Step A (RS4 §3): the call sites of a def's body, in source order, before any question of what they reach. Everything here is key-independent:
//! the engine's syntax and its macro expansion, never a type that depends on what a generic was given.
//!
//! A site is taken from the code the engine **lowered into the body**, never from the file's text: a `#[cfg(any())]` statement, block or match arm
//! inside an active body is text the engine has no expression for, and name resolution still answers there (ENGINE.md, "RS4: the bridge").
#![expect(
    dead_code,
    reason = "RS4's walk (steps 3 and 4) reads the sites and their nodes; step 2 proves the lists in the snapshot tests below"
)]

use std::collections::HashSet;
use std::rc::Rc;

use analyzer_core::graph::Site;
use analyzer_core::text::Span;
use ra_ap_hir::Macro;
use ra_ap_syntax::{AstNode, SyntaxNode, TextRange, WalkEvent, ast};

use super::bridge::{Func, Sem};

/// What kind of site (§3's table). `Passed` is step 6's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SiteKind {
    /// `f(a)`, `path::f(a)`, `(expr)(a)`, and `S(a)`, `Enum::V(a)`, `Self(a)`, which the resolver finds are constructions.
    Call,
    /// `x.m(a)`.
    MethodCall,
    /// `S { a }`, `Enum::V { a }`, `Self { .. }`.
    Record,
    /// A call that comes from the body of a **project** macro: at the macro call's path, and several of them share it (D30).
    MacroBody,
}

/// What the engine's syntax holds at a site, for step B to ask about. Private to `sem`: no engine type leaves it.
#[derive(Clone)]
enum Node {
    Call(ast::CallExpr),
    Method(ast::MethodCallExpr),
    Record(ast::RecordExpr),
}

/// One site of a body. `span` is relative to the def's `range.start` (core's `Site`).
#[derive(Clone)]
pub(crate) struct RawSite {
    kind: SiteKind,
    span: Site,
    /// Inside a closure or an `async` block: the number of the innermost one, in source order within the def, counted from 0. Layer 1 ignores it; RC2
    /// reads it.
    closure: Option<u32>,
    node: Node,
}

impl RawSite {
    pub(crate) fn kind(&self) -> SiteKind {
        self.kind
    }

    pub(crate) fn span(&self) -> Site {
        self.span
    }

    pub(crate) fn closure(&self) -> Option<u32> {
        self.closure
    }
}

/// The sites of one body, and what was lost on the way.
pub(crate) struct Sites {
    /// In source order: by where the site is (start, then end), and for sites at one place in the order the walk met them.
    pub(crate) list: Vec<RawSite>,
    /// Macro calls the engine could not expand: what was in their arguments is lost (§3), counted `macro_not_expanded`.
    pub(crate) macro_not_expanded: u32,
}

/// How deep a macro may expand into macros before the walk stops and counts it: the engine's own recursion limit is 128.
const MAX_MACRO_DEPTH: usize = 128;

/// The macro call whose expansion the walk is in: the innermost one decides `MacroBody` (§3).
#[derive(Clone, Copy)]
struct Expansion {
    /// The macro is the project's own (RS3 §4): its body is walked, and a call from it with no place in the file is a `MacroBody` site.
    project: bool,
    /// Where the macro call is written in the file, relative to the def: the path of the outermost call, or of the nearest one that has a place in the file.
    at: Site,
    depth: usize,
    /// The macro call, or one it is in the expansion of, is under a `#[cfg]` of the body: the code may be turned off.
    cfg: bool,
}

struct Walk<'a, 'p> {
    sem: &'a Sem<'p>,
    /// The def's `range.start` and `range.end`, in the file's bytes.
    def: Span,
    /// The def's file, as `Sem::open` numbers it: a range of the engine counts only in this one.
    file: u32,
    list: Vec<RawSite>,
    macro_not_expanded: u32,
    /// Closures and `async` blocks met so far, in source order.
    closures: u32,
}

impl<'p> Sem<'p> {
    /// The sites of `func`'s body: a function's block, a const's or static's initializer. A trait method with no body, a foreign function, a struct and an
    /// enum have none. Made once per request.
    pub(crate) fn sites(&self, func: Func<'p>) -> Rc<Sites> {
        if let Some(known) = self.sites_cache().borrow().get(&func) {
            return Rc::clone(known);
        }
        let made = Rc::new(self.collect_sites(func));
        self.sites_cache()
            .borrow_mut()
            .insert(func, Rc::clone(&made));
        made
    }

    fn collect_sites(&self, func: Func<'p>) -> Sites {
        let mut walk = Walk {
            sem: self,
            def: self.def_range(func),
            file: func.file(),
            list: Vec::new(),
            macro_not_expanded: 0,
            closures: 0,
        };
        if let Some(body) = self.body_of(func) {
            walk.node(&body, None, &mut Vec::new());
        }
        // source order (§3): where the site is, and for the ones at one place the order they were met. Stable, so a macro body's sites keep theirs.
        walk.list
            .sort_by_key(|site| (site.span.start, site.span.end));
        // A macro that repeats an argument (`both!(f())` → `(f(), f())`) makes one written call twice in the expansion: it is one site of the file (§3: two
        // sites never share a place and a kind), the first. A `MacroBody` site is the exception: several of them at one macro call are several targets.
        let mut seen = HashSet::new();
        walk.list.retain(|site| {
            site.kind == SiteKind::MacroBody
                || seen.insert((site.kind, site.span.start, site.span.end))
        });
        Sites {
            list: walk.list,
            macro_not_expanded: walk.macro_not_expanded,
        }
    }
}

impl Walk<'_, '_> {
    /// The sites of everything under `root` (the body, or a macro's expansion), nested items left out. `enclosing` holds the closures around the node being
    /// looked at. `expansion` is `None` in the file's own text.
    fn node(
        &mut self,
        root: &SyntaxNode,
        expansion: Option<Expansion>,
        enclosing: &mut Vec<(SyntaxNode, u32)>,
    ) {
        let mut events = root.preorder();
        while let Some(event) = events.next() {
            match event {
                WalkEvent::Enter(node) => {
                    if let Some(call) = ast::MacroCall::cast(node.clone()) {
                        events.skip_subtree();
                        self.macro_call(&call, root, expansion, enclosing);
                    } else if node != *root && ast::Item::can_cast(node.kind()) {
                        // a def of its own (RS1): walked when something calls it
                        events.skip_subtree();
                    } else if is_closure(&node) {
                        enclosing.push((node, self.closures));
                        self.closures += 1;
                    } else {
                        self.expression(&node, expansion, enclosing);
                    }
                }
                WalkEvent::Leave(node) => {
                    if enclosing.last().is_some_and(|(open, _)| *open == node) {
                        enclosing.pop();
                    }
                }
            }
        }
    }

    /// A macro call: its expansion in its place, so the order is the order the code runs in. The macro itself is not a node.
    fn macro_call(
        &mut self,
        call: &ast::MacroCall,
        root: &SyntaxNode,
        outer: Option<Expansion>,
        enclosing: &mut Vec<(SyntaxNode, u32)>,
    ) {
        // The macro call itself is not asked whether the engine lowered it: for one that expands to statements the answer is "no", as it is for a statement a
        // `cfg` turns off. The calls in its expansion are asked one by one (`expression`), which leaves nothing of a disabled statement.
        let depth = outer.map_or(0, |o| o.depth) + 1;
        let cfg = outer.is_some_and(|o| o.cfg) || under_cfg(call, root);
        let expanded = (depth <= MAX_MACRO_DEPTH)
            .then(|| self.sem.expand(call))
            .flatten();
        let Some(expanded) = expanded else {
            // The engine gives no expansion both for a macro it cannot expand and for one in a statement a `cfg` turns off (the inner `format_args_nl!` of a
            // disabled `println!`), and does not say which. Under a `#[cfg]` of the body it is the second, far more often: not counted. Anywhere else it
            // is a loss, counted.
            if !cfg {
                self.macro_not_expanded += 1;
            }
            return;
        };
        let path = call.path().map(|p| p.syntax().clone());
        // where the call is written: its path in the file, or (a call inside another macro's body) where the nearest enclosing one is
        let at = match (&path, outer) {
            (Some(path), None) => self.relative(span_of(path.text_range())),
            (Some(path), Some(outer)) => self
                .sem
                .original_range(path, self.file)
                .and_then(|range| self.within_def(range))
                .map_or(outer.at, |range| self.relative(range)),
            (None, Some(outer)) => outer.at,
            (None, None) => Site::at(0),
        };
        let project = self.sem.macro_of(call).is_some_and(|m| self.is_project(m));
        self.node(
            &expanded,
            Some(Expansion {
                project,
                at,
                depth,
                cfg,
            }),
            enclosing,
        );
    }

    /// The expression kinds that are sites.
    fn expression(
        &mut self,
        node: &SyntaxNode,
        expansion: Option<Expansion>,
        enclosing: &[(SyntaxNode, u32)],
    ) {
        let (shape, written) = if let Some(call) = ast::CallExpr::cast(node.clone()) {
            let Some(callee) = call.expr() else { return };
            (Node::Call(call), (SiteKind::Call, callee.syntax().clone()))
        } else if let Some(call) = ast::MethodCallExpr::cast(node.clone()) {
            let Some(name) = call.name_ref() else { return };
            (
                Node::Method(call),
                (SiteKind::MethodCall, name.syntax().clone()),
            )
        } else if let Some(record) = ast::RecordExpr::cast(node.clone()) {
            let Some(path) = record.path() else { return };
            (
                Node::Record(record),
                (SiteKind::Record, path.syntax().clone()),
            )
        } else {
            return;
        };
        let (kind, written) = written;
        // only what the engine lowered into the body: a call in a `#[cfg(any())]` statement is text it has no expression for
        if !self.lowered(node) {
            return;
        }
        let (kind, span) = match expansion {
            None => (kind, self.relative(span_of(written.text_range()))),
            Some(expansion) => match self
                .sem
                .original_range(&written, self.file)
                .and_then(|range| self.within_def(range))
            {
                // written in the file: an argument of the macro call
                Some(range) => (kind, self.relative(range)),
                // made by the macro's own body: a site only when the macro is the project's (D30), and then at the macro call
                None if expansion.project => (SiteKind::MacroBody, expansion.at),
                None => return,
            },
        };
        self.list.push(RawSite {
            kind,
            span,
            closure: enclosing.last().map(|(_, number)| *number),
            node: shape,
        });
    }

    fn relative(&self, range: Span) -> Site {
        Site::new(
            range.start.saturating_sub(self.def.start),
            range.end.saturating_sub(self.def.start),
        )
    }

    /// The range, when it is inside the def (a range that maps anywhere else is not a place in this body).
    fn within_def(&self, range: Span) -> Option<Span> {
        (self.def.start <= range.start && range.end <= self.def.end).then_some(range)
    }

    fn lowered(&self, node: &SyntaxNode) -> bool {
        ast::Expr::cast(node.clone()).is_some_and(|expr| self.sem.lowered(&expr))
    }

    fn is_project(&self, mac: Macro) -> bool {
        self.sem.is_project_macro(mac)
    }
}

fn span_of(range: TextRange) -> Span {
    Span::new(range.start().into(), range.end().into())
}

/// Whether the macro call, or something it stands in inside `root` (a statement, a block, a match arm), carries a `#[cfg(..)]`: the code may be turned off.
fn under_cfg(call: &ast::MacroCall, root: &SyntaxNode) -> bool {
    // by the tree and not by `HasAttrs`: which node an attribute hangs on (the macro call, its expression, the statement) is the parser's business
    call.syntax()
        .ancestors()
        .take_while(|node| node != root)
        .flat_map(|node| node.children())
        .filter_map(ast::Attr::cast)
        .any(|attr| attr.simple_name().is_some_and(|name| name == "cfg"))
}

/// A closure, or an `async` block: both are code written in the def that runs elsewhere (M13).
fn is_closure(node: &SyntaxNode) -> bool {
    ast::ClosureExpr::can_cast(node.kind())
        || ast::BlockExpr::cast(node.clone()).is_some_and(|block| block.async_token().is_some())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::sem::attached;
    use crate::sem::testing::{STYLE, disk_outline, disk_text, policy};

    /// One module's site lists, as a reviewable text: every def that can be an entry, with its sites in order. A site is its kind, its span relative to the
    /// def's start, the text there, and the closure it is in. What is lost to an expansion the engine could not make is printed too.
    fn render(sem: &Sem<'_>, root: &Path, file: &str) -> String {
        let text = disk_text(root, file);
        let outline = disk_outline(root, file);
        let mut out = String::new();
        for (def, occurrence) in outline.defs.iter().zip(rust_syntax::occurrences(&outline)) {
            let Some(func) = sem.entry(file, &def.qualname, occurrence) else {
                continue;
            };
            out.push_str(&def.qualname);
            if occurrence > 0 {
                out.push_str(&format!("#{occurrence}"));
            }
            out.push('\n');
            let sites = sem.sites(func);
            for site in &sites.list {
                let at = site.span();
                let from = (def.range.start + at.start) as usize;
                let to = (def.range.start + at.end) as usize;
                out.push_str(&format!(
                    "  {:?} {}..{} {:?}",
                    site.kind(),
                    at.start,
                    at.end,
                    &text[from..to]
                ));
                if let Some(closure) = site.closure() {
                    out.push_str(&format!(" closure {closure}"));
                }
                out.push('\n');
            }
            if sites.macro_not_expanded > 0 {
                out.push_str(&format!(
                    "  macro_not_expanded {}\n",
                    sites.macro_not_expanded
                ));
            }
        }
        out
    }

    /// RS4 §9 step 2's proof: the site list of every module of the policy fixture, with no targets. Snapshots are reviewed, not just accepted.
    #[test]
    fn the_site_list_of_every_policy_module() {
        let (loaded, root) = policy("sites_snapshots");
        let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/sites");
        attached(&loaded, STYLE, |sem| {
            for file in loaded.files() {
                let name = format!("{}.txt", file.replace('/', "__"));
                analyzer_testkit::expect_file(&snapshots.join(name), &render(sem, &root, &file));
            }
        });
    }

    /// The text of each site of `qualname` in `file`, in order, and the number of macro calls that were not expanded.
    fn site_texts(sem: &Sem<'_>, root: &Path, file: &str, qualname: &str) -> (Vec<String>, u32) {
        let text = disk_text(root, file);
        let outline = disk_outline(root, file);
        let def = outline
            .defs
            .iter()
            .find(|d| d.qualname == qualname)
            .unwrap_or_else(|| panic!("{qualname}"));
        let func = sem.entry(file, qualname, 0).unwrap();
        let sites = sem.sites(func);
        let texts = sites
            .list
            .iter()
            .map(|site| {
                let at = site.span();
                text[(def.range.start + at.start) as usize..(def.range.start + at.end) as usize]
                    .to_owned()
            })
            .collect();
        (texts, sites.macro_not_expanded)
    }

    /// The [check] of RS4 §3: a `#[cfg(any())]` statement, block, `let`, match arm, macro call, closure or record inside an active body gives no site, and
    /// is not a loss either. Name resolution answers there, so the sites are taken from what the engine lowered, not from the text.
    #[test]
    fn a_disabled_statement_block_or_arm_in_an_active_body_gives_no_site() {
        let (loaded, root) = policy("sites_cfg");
        attached(&loaded, STYLE, |sem| {
            let file = "src/cfg_off.rs";
            let sites = |qualname: &str| site_texts(sem, &root, file, qualname);
            // the statement, the block and the macro call each have a disabled call and an active one: only the active one is a site
            for qualname in ["statement", "block", "macro_statement"] {
                assert_eq!(sites(qualname), (vec!["f".to_owned()], 0), "{qualname}");
            }
            assert_eq!(sites("method_statement"), (vec!["len".to_owned()], 0));
            assert_eq!(
                sites("struct_expression"),
                (vec!["crate::shapes::Rec".to_owned()], 0)
            );
            // nothing active at all
            for qualname in ["let_statement", "match_arm", "closure_statement"] {
                assert_eq!(sites(qualname), (vec![], 0), "{qualname}");
            }
            // and an enabled `#[cfg]` is not a disabled one
            assert_eq!(sites("on_statement"), (vec!["f".to_owned()], 0));
        });
    }

    /// §3's table, row by row, in the one place a reader looks for it: what is a site and what is not, with the closure each is in.
    #[test]
    fn the_rows_of_the_table() {
        let (loaded, root) = policy("sites_rows");
        attached(&loaded, STYLE, |sem| {
            let calls = |qualname: &str| site_texts(sem, &root, "src/calls.rs", qualname).0;
            // a unit struct and a unit variant named as values are no call; a tuple struct, a tuple variant and a record are
            assert_eq!(calls("constructions"), ["Pair", "E::V", "Rec", "E::R"]);
            // operators, `?` and `for` are not sites; `Some(v)` is
            assert_eq!(calls("operators"), ["Some"]);
            // a callee that is not a path is still a site, at the whole callee expression
            assert_eq!(calls("callee_expressions"), ["(h.cb)", "f", "(helper)"]);
            // source order is where the site is: `twice` before `get` before `helper`, not the nesting of the calls
            assert_eq!(calls("order"), ["twice", "get", "helper"]);
            // a nested item is a def of its own: not the outer function's sites, and a site in each of them
            assert_eq!(calls("nested_items"), ["inner"]);
            assert_eq!(calls("nested_items::{fn}::inner"), ["helper"]);
            assert_eq!(calls("nested_items::{fn}::N"), ["helper_const"]);
            assert_eq!(calls("nested_items::{fn}::Local::method"), ["helper"]);
            // a const's and a static's initializer is a body; a trait method without a default and a struct have none
            assert_eq!(calls("FROM_CONST"), ["helper_const"]);
            assert_eq!(calls("FROM_STATIC"), ["helper_const"]);
            assert_eq!(calls("Declared::declared"), Vec::<String>::new());
            assert_eq!(calls("Wrapper"), Vec::<String>::new());

            // closures and async blocks are numbered in source order within the def, the innermost one tags a site, and a site outside any is untagged
            let func = sem.entry("src/calls.rs", "closures", 0).unwrap();
            let tags: Vec<_> = sem.sites(func).list.iter().map(RawSite::closure).collect();
            assert_eq!(
                tags,
                [Some(0), Some(1), Some(1), Some(2), None, None],
                "`|| helper()`, then `|x| twice(x) + (|| helper())()` with the inner one nested, then `a()` and `b(1)`"
            );
            let func = sem.entry("src/calls.rs", "asynchronous", 0).unwrap();
            let tags: Vec<_> = sem.sites(func).list.iter().map(RawSite::closure).collect();
            assert_eq!(
                tags,
                [Some(0), None],
                "an `async` block is a closure; the body of an `async fn` is not"
            );
        });
    }

    /// §3's macro rows: a call written in a macro's arguments is where it is written; a call from a project macro's body is at the macro call (the
    /// outermost one written in the file); a call from an external macro's body, even one inside a project macro, is never a site.
    #[test]
    fn macros_are_expanded_and_the_innermost_one_decides() {
        let (loaded, root) = policy("sites_macros");
        attached(&loaded, STYLE, |sem| {
            let kinds = |qualname: &str| {
                let func = sem.entry("src/macros.rs", qualname, 0).unwrap();
                let sites = sem.sites(func);
                sites
                    .list
                    .iter()
                    .map(|s| s.kind())
                    .collect::<Vec<SiteKind>>()
            };
            let texts = |qualname: &str| site_texts(sem, &root, "src/macros.rs", qualname);
            // `println!`, `vec!`, `assert!`: the calls written in the arguments, and none of the macros' own
            assert_eq!(texts("std_arguments").0, ["f", "g", "f"]);
            assert_eq!(kinds("std_arguments"), [SiteKind::Call; 3]);
            // a project macro's body: one site per call, at the macro call, kind `MacroBody`
            assert_eq!(texts("project_body").0, ["log"]);
            assert_eq!(kinds("project_body"), [SiteKind::MacroBody]);
            // the project macro's body holds `println!`, an external macro: its `_print` is not a site, the argument `f()` is
            assert_eq!(texts("project_wrapping_external").0, ["f"]);
            assert_eq!(kinds("project_wrapping_external"), [SiteKind::Call]);
            // a macro in a macro's body: the outermost call written in the file
            assert_eq!(texts("macro_in_macro").0, ["nested"]);
            // an argument the macro uses twice is one site (a place and a kind are one site), and a body that makes two calls is two at one place
            assert_eq!(texts("argument_twice").0, ["f"]);
            assert_eq!(
                texts("statements_in_a_body").0,
                ["statements", "statements", "f"]
            );
            assert_eq!(
                kinds("statements_in_a_body"),
                [SiteKind::MacroBody, SiteKind::MacroBody, SiteKind::Call]
            );
            // a project macro defined in another module: the call of its body is at the macro call, and its argument is a call where it is written
            let file = "src/calls.rs";
            let (written, _) = site_texts(sem, &root, file, "macro_from_another_module");
            assert_eq!(written, ["from_elsewhere", "helper"]);
            let func = sem.entry(file, "macro_from_another_module", 0).unwrap();
            let kinds: Vec<_> = sem.sites(func).list.iter().map(RawSite::kind).collect();
            assert_eq!(kinds, [SiteKind::MacroBody, SiteKind::Call]);
            // what the engine cannot expand is counted, and what was in its arguments is lost
            assert_eq!(texts("not_expanded"), (vec!["g".to_owned()], 1));
        });
    }

    #[test]
    fn the_sites_of_a_def_are_made_once_per_request() {
        let (loaded, _root) = policy("sites_cache");
        attached(&loaded, STYLE, |sem| {
            let func = sem.entry("src/calls.rs", "plain_calls", 0).unwrap();
            assert!(Rc::ptr_eq(&sem.sites(func), &sem.sites(func)));
        });
    }
}
