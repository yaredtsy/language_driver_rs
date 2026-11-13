//! A coverage test for rule 8 (`docs/03-typescript/07-ts5-driver-needs.md` §7, row 3c): **every call-like node of a project's source is drawn in some tree
//! or counted in some counter**, or it is on a list of known gaps, each with a reason that the owner decides.
//!
//! The oracle is an independent scan. It walks every node of every file with the engine's own child iteration and reads the grammar for what is call-like
//! (`anchor_of`). It does not call or copy `Collector`, `body_of`, `def_of`, `is_plain_call`, `is_callee_position` or `call_sites`: had it shared any of them
//! it would agree with the walker by construction and prove nothing. What it shares with the walker is the measure of a span (`own_span`, the convention that
//! lets a span from `typescript-syntax` find a node here), the engine's flag for ambient code, and `is_string_literal_like`.
//!
//! What is compared with the scan is what the analyzer **does**: the graph of every def of the project taken as an entry (a class as its construction, as a
//! request does), whose edges say where a site is drawn, and the sites the four counters report. The comparison is by span, one way: a scanned node must be
//! in drawn ∪ counted. It is not asserted the other way round, because an accessor site comes from the checker and a syntax scan cannot see it; how many
//! drawn spans the scan does not have is printed by edge kind instead.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use analyzer_core::graph::{EdgeTo, Graph};
use analyzer_core::outline::DefKind;
use analyzer_core::solve::{Budget, Cancel, Limits, Solver};
use typescript_syntax::Placement;

use super::collect::SiteKind;
use super::*;
use crate::calls::{TypeScript, start_of};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

// The categories of a miss, from its syntactic position.
const TOP_LEVEL: &str = "top-level";
const TOP_LEVEL_ANONYMOUS: &str = "top-level-anonymous";
const ENUM_MEMBER: &str = "enum-member";
const NESTED_CLASS_DEFINITION: &str = "nested-class-definition";
const NAMESPACE_CLASS_COMPUTED_NAME: &str = "namespace-class-computed-name";
const NAMESPACE_LEVEL: &str = "namespace-level";
// A miss in one of these is a bug and not a gap: nothing on the list may name them.
const TOP_CLASS_DEFINITION: &str = "top-class-definition";
const TOP_CLASS_COMPUTED_NAME: &str = "top-class-computed-name";
const NESTED_CLASS_COMPUTED_NAME: &str = "nested-class-computed-name";
const INSTANCE_INITIALIZER: &str = "instance-initializer";
const IN_NAMED_FUNCTION: &str = "in-named-function";
const OTHER: &str = "other";

/// The known gaps: (category, why nothing draws or counts it, what owns the decision). A miss in a category that is not here fails the test, and so does
/// a category here that no fixture hits (`the_list_has_no_stale_entry`): a stale entry hides nothing, but it lies, and is removed when its gap is fixed.
const KNOWN_GAPS: &[(&str, &str, &str)] = &[
    (
        TOP_LEVEL,
        "a call at the top of a file (`const x = f();`, `f();`, `export default g();`): no def is around it, and nothing walks a module's own body",
        "TS5 row 3c: expected by the row; the owner decides whether a file's top level is something a request can start from",
    ),
    (
        TOP_LEVEL_ANONYMOUS,
        "a call inside a callback or an IIFE at the top of a file: a call in an anonymous function belongs to the def around it (phase 1), and there is none",
        "TS5 row 3c: the same decision as `top-level`",
    ),
    (
        ENUM_MEMBER,
        "`enum E { A = f() }`: `body_of` says an enum has nothing to walk, and no counter is for it",
        "TS5 row 3c: found by the test; the owner decides",
    ),
    (
        NESTED_CLASS_DEFINITION,
        "a static field initializer, a static block, the `extends` expression or a decorator of a class declared in a namespace (or an `extends`, a static initializer or a static block of one in a function): the def around it stops at the class, and `definition_sites` is for a class at the top of a file",
        "TS5 row 3b",
    ),
    (
        NAMESPACE_CLASS_COMPUTED_NAME,
        "`[key()]` of a class declared in a namespace: `computed_name_sites` is for a class at the top of a file, and nothing walks a namespace",
        "TS5 row 3b",
    ),
    (
        NAMESPACE_LEVEL,
        "a call in a namespace body outside any function: a namespace is not a def a request can start from, and nothing walks it",
        "TS5 row 3c: found by the test; the owner decides, with `top-level`",
    ),
];

/// A node the scan leaves out of the comparison, by the policy that says it is not a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Policy {
    /// `import("./m")` and `require("./m")` with a literal: a module load, not a call into a function.
    ModuleLoad,
    /// `<div>`, `<my-el>`, `<svg:rect>`: a tag that names no value, so there is no def to call.
    IntrinsicTag,
    /// Anything in ambient code (`declare …`, a `.d.ts` file): it never runs.
    Ambient,
}

/// What became of one call-like node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Excluded(Policy),
    /// An edge of the graph is on the node's anchor.
    Drawn,
    /// One of the four counters reports the node (which one).
    Counted(&'static str),
    /// Neither: a miss, in this category.
    Missed(&'static str),
}

/// One call-like node of the source.
#[derive(Clone, Debug)]
struct Found {
    file: String,
    /// The anchor: the span the walker would give the site (`own_span` of the callee, the class, the tag, the decorator's expression).
    span: Span,
    what: &'static str,
    line: usize,
    /// The anchor's text, on one line, for the examples.
    text: String,
    outcome: Outcome,
}

/// The call-like nodes the scan looks for, with the node whose span anchors each (the plan's convention for a site).
fn anchor_of(node: P<Node>) -> Option<(&'static str, P<Node>)> {
    match node.kind() {
        // `f(x)`, `a?.b(x)`, `super(x)`, `import(x)`
        Kind::CallExpression => Some(("call", node.expression()?)),
        Kind::NewExpression => Some(("new", node.expression()?)),
        Kind::TaggedTemplateExpression => {
            Some(("tagged", node.as_tagged_template_expression().tag))
        }
        // the closing tag is the same element
        Kind::JsxOpeningElement | Kind::JsxSelfClosingElement => Some(("jsx", node.tag_name())),
        Kind::Decorator => Some(("decorator", node.expression()?)),
        _ => None,
    }
}

/// The policy that leaves a node out, if one does.
fn policy_of(node: P<Node>, what: &str, anchor: P<Node>) -> Option<Policy> {
    // the engine's flag: set on everything in a declaration file and on everything under a `declare`
    if node.flags().intersects(NodeFlags::Ambient) {
        return Some(Policy::Ambient);
    }
    match what {
        "call" if anchor.kind() == Kind::ImportKeyword => Some(Policy::ModuleLoad),
        "call"
            if anchor.kind() == Kind::Identifier
                && anchor.text() == "require"
                && matches!(node.arguments(), [argument] if ast::is_string_literal_like(*argument)) =>
        {
            Some(Policy::ModuleLoad)
        }
        "jsx" if is_intrinsic(anchor) => Some(Policy::IntrinsicTag),
        _ => None,
    }
}

/// JSX: a namespaced name, an identifier that starts in lower case or holds a dash. A member access (`<a.B />`) and `this` name a value.
fn is_intrinsic(tag: P<Node>) -> bool {
    match tag.kind() {
        Kind::JsxNamespacedName => true,
        Kind::Identifier => {
            let name = tag.text();
            name.starts_with(char::is_lowercase) || name.contains('-')
        }
        _ => false,
    }
}

/// Where a miss is, in a category: found by walking up the parents. An enum member, the definition time of a class (and where that class is), the body of a
/// namespace, or the top of a file. A function that a def of the outline stands for is **named** here by the grammar (a declaration, a method, a named
/// function expression, an arrow held by a `const`, a property or a class field): a miss in one is a bug, and is its own category.
fn category_of(node: P<Node>) -> &'static str {
    let mut chain = vec![node];
    while let Some(parent) = chain.last().and_then(|last| last.parent()) {
        chain.push(parent);
    }
    // passed through an anonymous function or class expression, whose calls belong to the def around it
    let mut anonymous = false;
    for index in 1..chain.len() {
        let ancestor = chain[index];
        let below = chain[index - 1];
        let part = index.checked_sub(2).map(|at| chain[at]);
        let part_of_part = index.checked_sub(3).map(|at| chain[at]);
        match ancestor.kind() {
            Kind::EnumMember => return ENUM_MEMBER,
            kind if is_function(kind) => {
                // what runs when a class is defined is not in the function's body: the decorator of the member or of a parameter, a computed name
                let of_a_class = ancestor.parent().is_some_and(|p| is_class(p.kind()));
                let parameter_decorator = below.kind() == Kind::Parameter
                    && part.is_some_and(|p| p.kind() == Kind::Decorator);
                if of_a_class
                    && (below.kind() == Kind::Decorator
                        || below.kind() == Kind::ComputedPropertyName
                        || parameter_decorator)
                {
                    continue;
                }
                if is_named_function(ancestor) {
                    return IN_NAMED_FUNCTION;
                }
                anonymous = true;
            }
            kind if is_class(kind) => {
                if !is_named_class(ancestor) {
                    anonymous = true;
                    continue;
                }
                let position = match below.kind() {
                    Kind::HeritageClause | Kind::Decorator | Kind::ClassStaticBlockDeclaration => {
                        Position::Definition
                    }
                    // `below` is a member
                    _ => match part.map(|p| p.kind()) {
                        Some(Kind::Decorator) => Position::Definition,
                        Some(Kind::ComputedPropertyName) => Position::ComputedName,
                        Some(Kind::Parameter)
                            if part_of_part.is_some_and(|p| p.kind() == Kind::Decorator) =>
                        {
                            Position::Definition
                        }
                        _ if below.kind() == Kind::PropertyDeclaration && ast::is_static(below) => {
                            Position::Definition
                        }
                        _ if below.kind() == Kind::PropertyDeclaration => Position::Instance,
                        _ => return OTHER,
                    },
                };
                return place_of_class(ancestor, position);
            }
            Kind::ModuleDeclaration => return NAMESPACE_LEVEL,
            _ => {}
        }
    }
    if anonymous {
        TOP_LEVEL_ANONYMOUS
    } else {
        TOP_LEVEL
    }
}

/// Which part of a class a node is in.
#[derive(Clone, Copy)]
enum Position {
    /// What runs when the class is defined.
    Definition,
    ComputedName,
    /// An instance field initializer: a construction runs it, and `initializer_sites` counts it.
    Instance,
}

/// Where the class is, which says who would draw or count what is in the position.
fn place_of_class(class: P<Node>, position: Position) -> &'static str {
    enum Place {
        Top,
        Function,
        Namespace,
    }
    let mut place = Place::Top;
    let mut up = class.parent();
    while let Some(parent) = up {
        let kind = parent.kind();
        if is_function(kind) || is_class(kind) {
            place = Place::Function;
            break;
        }
        if kind == Kind::ModuleDeclaration {
            place = Place::Namespace;
            break;
        }
        up = parent.parent();
    }
    match (position, place) {
        (Position::Instance, _) => INSTANCE_INITIALIZER,
        (Position::Definition, Place::Top) => TOP_CLASS_DEFINITION,
        (Position::Definition, _) => NESTED_CLASS_DEFINITION,
        (Position::ComputedName, Place::Top) => TOP_CLASS_COMPUTED_NAME,
        (Position::ComputedName, Place::Namespace) => NAMESPACE_CLASS_COMPUTED_NAME,
        (Position::ComputedName, Place::Function) => NESTED_CLASS_COMPUTED_NAME,
    }
}

fn is_function(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::FunctionDeclaration
            | Kind::FunctionExpression
            | Kind::ArrowFunction
            | Kind::MethodDeclaration
            | Kind::Constructor
            | Kind::GetAccessor
            | Kind::SetAccessor
    )
}

fn is_class(kind: Kind) -> bool {
    matches!(kind, Kind::ClassDeclaration | Kind::ClassExpression)
}

/// A function that is a def by the grammar: it has a name, or it is an arrow or function expression that something names: a `const`, a class field with a
/// plain name, or a property of an object literal that a `const` holds (a property of any other literal is not a def: its calls belong to the def around it).
fn is_named_function(function: P<Node>) -> bool {
    match function.kind() {
        Kind::ArrowFunction | Kind::FunctionExpression => {
            function.name().is_some()
                || function.parent().is_some_and(|parent| match parent.kind() {
                    Kind::VariableDeclaration => ast::is_var_const(parent),
                    Kind::PropertyDeclaration => names_a_def(parent),
                    Kind::PropertyAssignment => names_a_def(parent) && is_in_const_literal(parent),
                    _ => false,
                })
        }
        _ => true,
    }
}

/// A member whose name is an identifier or a string literal: a computed name, a number or a private name names no def.
fn names_a_def(member: P<Node>) -> bool {
    member
        .name()
        .is_some_and(|name| matches!(name.kind(), Kind::Identifier | Kind::StringLiteral))
}

/// A property of an object literal that a `const` holds, directly or through a nested literal, `as const`, `satisfies` or parentheses.
fn is_in_const_literal(property: P<Node>) -> bool {
    let mut literal = property.parent();
    while let Some(object) = literal.filter(|o| o.kind() == Kind::ObjectLiteralExpression) {
        let mut holder = object.parent();
        while let Some(wrapper) = holder.filter(|h| {
            matches!(
                h.kind(),
                Kind::AsExpression
                    | Kind::SatisfiesExpression
                    | Kind::TypeAssertionExpression
                    | Kind::ParenthesizedExpression
            )
        }) {
            holder = wrapper.parent();
        }
        match holder {
            Some(holder) if holder.kind() == Kind::VariableDeclaration => {
                return ast::is_var_const(holder);
            }
            Some(holder) if holder.kind() == Kind::PropertyAssignment => {
                literal = holder.parent();
            }
            _ => return false,
        }
    }
    false
}

fn is_named_class(class: P<Node>) -> bool {
    class.kind() == Kind::ClassDeclaration
        || class.name().is_some()
        || class
            .parent()
            .is_some_and(|parent| parent.kind() == Kind::VariableDeclaration)
}

/// Every call-like node of one file, with its anchor, each with the outcome the **scan** can decide (an exclusion) or the category it would have if missed.
fn scan(file: File<'_>, relative: &str) -> Vec<(Found, Option<Policy>, &'static str)> {
    let text = file.node.text();
    let mut found = Vec::new();
    let mut pending = vec![file.node.as_node()];
    while let Some(node) = pending.pop() {
        node.for_each_child(&mut |child| {
            pending.push(child);
            false
        });
        let Some((what, anchor)) = anchor_of(node) else {
            continue;
        };
        let span = own_span(text, anchor);
        let policy = policy_of(node, what, anchor);
        let category = if policy.is_none() {
            category_of(node)
        } else {
            OTHER
        };
        let line = text[..span.start as usize].matches('\n').count() + 1;
        let shown: String = text[span.start as usize..span.end as usize]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        found.push((
            Found {
                file: relative.to_owned(),
                span,
                what,
                line,
                text: shown.chars().take(60).collect(),
                outcome: Outcome::Missed(OTHER),
            },
            policy,
            category,
        ));
    }
    found.sort_by_key(|(found, ..)| (found.span.start, found.span.end));
    found
}

/// Where each edge of the graph is, by the file and the absolute span of its site (the context's def start plus the site), with the kind of each edge.
fn drawn(graph: &Graph) -> HashMap<(String, Span), Vec<&'static str>> {
    let mut found: HashMap<(String, Span), Vec<&'static str>> = HashMap::new();
    for edge in &graph.edges {
        let def = graph
            .def(&edge.from.func)
            .expect("an edge's context has a def on the graph");
        let at = def.range.start;
        let span = Span::new(at + edge.site.start, at + edge.site.end);
        let kind = match (&edge.to, edge.passed) {
            (EdgeTo::Context(..), true) => "passed",
            (EdgeTo::Context(..), false) => "call",
            (EdgeTo::Construct { .. }, _) => "construct",
            (EdgeTo::External(..), _) => "external",
            (EdgeTo::Unresolved(..), _) => "unresolved",
            _ => "other",
        };
        found
            .entry((def.file.clone(), span))
            .or_default()
            .push(kind);
    }
    found
}

impl Sem {
    /// The sites the four counters report for `defs`, as the walk notes them: for a class taken as a construction, the instance initializers of each class
    /// the construction goes through, and the definition sites and the computed names of the class (`walk::construction`); for every other def, the decorators
    /// of the classes its body declares (`handed_over`). By file and span, with the counter's name.
    fn counted_sites(&self, defs: &[Func<'_>]) -> HashMap<(String, Span), &'static str> {
        let mut found = HashMap::new();
        let mut put = |class: Func<'_>, spans: Vec<Span>, counter: &'static str| {
            let file = self.describe(class, STYLE).file;
            for span in spans {
                found.insert((file.clone(), span), counter);
            }
        };
        for &func in defs {
            if self.def(func, STYLE).kind == DefKind::Class {
                let (_, chain) = self.init_chain(func, STYLE);
                for class in chain {
                    put(class, self.initializer_sites(class, STYLE), "initializer");
                }
                put(func, self.definition_sites(func, STYLE), "definition");
                put(func, self.computed_name_sites(func, STYLE), "computed-name");
            } else if let Some(body) = self.body(func, STYLE) {
                let Some(defs) = self.file_defs(func.file, STYLE) else {
                    continue;
                };
                for &(def, _) in &body.nested {
                    if defs.outline.defs[def].kind == DefKind::Class {
                        let class = Func {
                            file: func.file,
                            def,
                        };
                        put(class, self.decorator_sites(class, STYLE), "decorator");
                    }
                }
            }
        }
        found
    }

    /// The sites a body has that come from the checker (an accessor): a syntax scan cannot see them.
    fn accessor_sites_of(&self, defs: &[Func<'_>]) -> HashSet<(String, Span)> {
        let mut found = HashSet::new();
        for &func in defs {
            let Some(body) = self.body(func, STYLE) else {
                continue;
            };
            let file = self.describe(func, STYLE).file;
            for site in &body.sites {
                if matches!(site.kind, SiteKind::Accessor(_)) {
                    found.insert((file.clone(), site.span));
                }
            }
        }
        found
    }
}

/// What the scan found in one program, and how it compares with the graph.
struct Analysis {
    found: Vec<Found>,
    /// The files scanned, relative to the root.
    files: Vec<String>,
    /// Drawn spans no scanned node has, by edge kind (`accessor` for a site the checker found).
    drawn_not_scanned: BTreeMap<&'static str, usize>,
    /// Nodes whose anchor is not a drawn or counted span, but whose **start** is one: the spans did not line up exactly. 0 when they do.
    start_only: usize,
    /// Scanned nodes that share an anchor span with another one.
    shared_anchors: usize,
    /// The graph had a cut, so a missing edge may be a limit and not a gap.
    cuts: u64,
}

/// Scan every project file of `sem`, and compare it with the graph of every def of the project taken as an entry.
fn analyze(sem: &Sem) -> Analysis {
    let defs = sem.project_defs(STYLE);
    let lang = TypeScript::new(sem, STYLE);
    let entries: Vec<_> = defs.iter().map(|&func| start_of(&lang, func)).collect();
    let mut solver = Solver::new(lang, Limits::new(u32::MAX, None));
    let graph = solver.graph_within(&entries, &Budget::default(), &Cancel::new());
    let drawn = drawn(&graph);
    let counted = sem.counted_sites(&defs);
    let accessors = sem.accessor_sites_of(&defs);

    let mut found = Vec::new();
    let mut files = Vec::new();
    let mut anchors: HashMap<(String, Span), usize> = HashMap::new();
    for file in sem.files() {
        if !(sem.is_project_code(file) || sem.is_repo_declaration_file(file)) {
            continue;
        }
        let Some(relative) = sem.relative_path(file) else {
            continue;
        };
        files.push(relative.clone());
        for (mut node, policy, category) in scan(file, &relative) {
            let key = (node.file.clone(), node.span);
            *anchors.entry(key.clone()).or_default() += 1;
            node.outcome = if let Some(policy) = policy {
                Outcome::Excluded(policy)
            } else if drawn.contains_key(&key) {
                Outcome::Drawn
            } else if let Some(counter) = counted.get(&key) {
                Outcome::Counted(counter)
            } else {
                Outcome::Missed(category)
            };
            found.push(node);
        }
    }
    let starts: HashSet<(&str, u32)> = drawn
        .keys()
        .chain(counted.keys())
        .map(|(file, span)| (file.as_str(), span.start))
        .collect();
    let start_only = found
        .iter()
        .filter(|node| matches!(node.outcome, Outcome::Missed(_)))
        .filter(|node| starts.contains(&(node.file.as_str(), node.span.start)))
        .count();
    let mut drawn_not_scanned: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (key, kinds) in &drawn {
        if anchors.contains_key(key) {
            continue;
        }
        for kind in kinds {
            let kind = if accessors.contains(key) {
                "accessor"
            } else {
                kind
            };
            *drawn_not_scanned.entry(kind).or_default() += 1;
        }
    }
    Analysis {
        found,
        files,
        drawn_not_scanned,
        start_only,
        shared_anchors: anchors.values().filter(|&&n| n > 1).count(),
        cuts: graph.stats.cuts.values().sum(),
    }
}

/// The numbers of one file, as the table of expectations writes them.
#[derive(Debug, Default, PartialEq, Eq)]
struct Numbers {
    drawn: usize,
    counted: usize,
    excluded: usize,
    missed: BTreeMap<&'static str, usize>,
}

fn numbers_by_file(found: &[Found]) -> BTreeMap<String, Numbers> {
    let mut files: BTreeMap<String, Numbers> = BTreeMap::new();
    for node in found {
        let numbers = files.entry(node.file.clone()).or_default();
        match node.outcome {
            Outcome::Drawn => numbers.drawn += 1,
            Outcome::Counted(_) => numbers.counted += 1,
            Outcome::Excluded(_) => numbers.excluded += 1,
            Outcome::Missed(category) => *numbers.missed.entry(category).or_default() += 1,
        }
    }
    files
}

/// What one file of `tests/fixtures/coverage` must read. Every node a file holds is accounted for, so the scanned number is the sum.
struct Expect {
    file: &'static str,
    drawn: usize,
    counted: usize,
    excluded: usize,
    missed: &'static [(&'static str, usize)],
}

/// Written before the first run.
const EXPECTED: &[Expect] = &[
    Expect {
        file: "src/ambient.d.ts",
        drawn: 0,
        counted: 0,
        excluded: 1,
        missed: &[],
    },
    Expect {
        file: "src/defaults.ts",
        drawn: 5,
        counted: 0,
        excluded: 0,
        missed: &[],
    },
    Expect {
        file: "src/enum.ts",
        drawn: 0,
        counted: 0,
        excluded: 0,
        missed: &[(ENUM_MEMBER, 3)],
    },
    Expect {
        file: "src/excluded.ts",
        drawn: 1,
        counted: 0,
        excluded: 3,
        missed: &[],
    },
    Expect {
        file: "src/iife.ts",
        drawn: 5,
        counted: 0,
        excluded: 0,
        missed: &[(TOP_LEVEL, 2), (TOP_LEVEL_ANONYMOUS, 2)],
    },
    Expect {
        file: "src/jsx.tsx",
        drawn: 2,
        counted: 0,
        excluded: 3,
        missed: &[(TOP_LEVEL, 1)],
    },
    Expect {
        file: "src/namespace-class.ts",
        drawn: 2,
        counted: 1,
        excluded: 0,
        missed: &[
            (NAMESPACE_CLASS_COMPUTED_NAME, 1),
            (NAMESPACE_LEVEL, 1),
            (NESTED_CLASS_DEFINITION, 2),
        ],
    },
    Expect {
        file: "src/nested-class.ts",
        drawn: 5,
        counted: 4,
        excluded: 0,
        missed: &[(NESTED_CLASS_DEFINITION, 3)],
    },
    Expect {
        file: "src/tagged.ts",
        drawn: 2,
        counted: 0,
        excluded: 0,
        missed: &[(TOP_LEVEL, 2)],
    },
    Expect {
        file: "src/top-class.ts",
        drawn: 4,
        counted: 8,
        excluded: 0,
        missed: &[],
    },
    Expect {
        file: "src/top-level.ts",
        drawn: 0,
        counted: 0,
        excluded: 0,
        missed: &[(TOP_LEVEL, 5)],
    },
];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn analyze_project(root: &Path, file: &str) -> Analysis {
    let sem = Sem::open(root, Path::new(file)).expect("the fixture opens");
    analyze(&sem)
}

#[test]
fn each_coverage_fixture_has_its_nodes_accounted_for() {
    let analysis = analyze_project(&fixtures().join("coverage"), "src/top-level.ts");
    assert_eq!(analysis.cuts, 0, "a cut graph proves nothing about a gap");
    let want: BTreeMap<String, Numbers> = EXPECTED
        .iter()
        .map(|expect| {
            (
                expect.file.to_owned(),
                Numbers {
                    drawn: expect.drawn,
                    counted: expect.counted,
                    excluded: expect.excluded,
                    missed: expect.missed.iter().copied().collect(),
                },
            )
        })
        .collect();
    let got = numbers_by_file(&analysis.found);
    let differing: Vec<String> = want
        .keys()
        .chain(got.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|file| want.get(*file) != got.get(*file))
        .map(|file| {
            format!(
                "{file}\n  wanted {:?}\n  read   {:?}",
                want.get(file),
                got.get(file)
            )
        })
        .collect();
    assert!(
        differing.is_empty(),
        "files whose numbers differ:\n{}",
        differing.join("\n")
    );
    // the anchors line up exactly, trivia included: no miss has a drawn or counted span starting where it does
    assert_eq!(analysis.start_only, 0);
    assert_eq!(analysis.shared_anchors, 0);
}

/// The TypeScript and JavaScript sources under `dir`, sorted, not in `node_modules` and not declarations.
fn source_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if name != "node_modules" {
                    pending.push(path);
                }
            } else if name.ends_with(".d.ts")
                || name.ends_with(".d.mts")
                || name.ends_with(".d.cts")
            {
                continue;
            } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                matches!(
                    e,
                    "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs"
                )
            }) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Every call-like node of the projects in `dir`: each source file opens the program that owns it, unless an earlier one already scanned that file. A file
/// no program holds (a broken config, a file the config leaves out) is in `skipped`. Files are named `label/relative`.
fn sweep(label: &str, dir: &Path, found: &mut Vec<Found>, skipped: &mut Vec<String>) -> usize {
    let mut seen: HashSet<String> = HashSet::new();
    let mut opened = 0;
    for source in source_files(dir) {
        let relative = source
            .strip_prefix(dir)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if seen.contains(&relative) {
            continue;
        }
        let Ok(sem) = Sem::open(dir, Path::new(&relative)) else {
            skipped.push(format!("{label}/{relative}"));
            continue;
        };
        opened += 1;
        let analysis = analyze(&sem);
        assert_eq!(analysis.cuts, 0, "{label}/{relative}: the graph was cut");
        seen.extend(analysis.files);
        for mut node in analysis.found {
            node.file = format!("{label}/{}", node.file);
            found.push(node);
        }
    }
    opened
}

/// The shared scenarios written in TypeScript: `scenarios/<name>/typescript`.
fn scenario_dirs() -> Vec<(String, PathBuf)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../scenarios");
    let mut dirs: Vec<(String, PathBuf)> = std::fs::read_dir(&root)
        .expect("the scenarios are in the repo")
        .flatten()
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.path().join("typescript"),
            )
        })
        .filter(|(_, dir)| dir.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Every project under `tests/fixtures` and every scenario, scanned.
fn sweep_everything() -> (Vec<Found>, Vec<String>, usize) {
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    let mut opened = 0;
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(fixtures())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let label = dir.file_name().unwrap().to_string_lossy().into_owned();
        opened += sweep(&label, &dir, &mut found, &mut skipped);
    }
    for (name, dir) in scenario_dirs() {
        opened += sweep(&format!("scenario {name}"), &dir, &mut found, &mut skipped);
    }
    (found, skipped, opened)
}

fn missed(found: &[Found]) -> BTreeMap<&'static str, Vec<&Found>> {
    let mut by_category: BTreeMap<&'static str, Vec<&Found>> = BTreeMap::new();
    for node in found {
        if let Outcome::Missed(category) = node.outcome {
            by_category.entry(category).or_default().push(node);
        }
    }
    by_category
}

#[test]
fn every_miss_in_every_fixture_and_scenario_is_in_a_listed_category() {
    let (found, skipped, opened) = sweep_everything();
    // a sweep that opened nothing proves nothing
    assert!(
        opened >= 25,
        "only {opened} programs opened, skipped {skipped:?}"
    );
    let listed: HashSet<&str> = KNOWN_GAPS.iter().map(|(category, ..)| *category).collect();
    let unlisted: Vec<String> = missed(&found)
        .into_iter()
        .filter(|(category, _)| !listed.contains(category))
        .flat_map(|(category, nodes)| {
            nodes.into_iter().map(move |n| {
                format!(
                    "{category}: {}:{} `{}` ({})",
                    n.file, n.line, n.text, n.what
                )
            })
        })
        .collect();
    assert!(
        unlisted.is_empty(),
        "misses in a category that is not on the list:\n{}",
        unlisted.join("\n")
    );
    let by_category: Vec<String> = missed(&found)
        .into_iter()
        .map(|(category, nodes)| {
            // the sites outside `coverage`, whose fixtures were written for the categories
            let elsewhere: Vec<String> = nodes
                .iter()
                .filter(|node| !node.file.starts_with("coverage/"))
                .map(|node| format!("{}:{}", node.file, node.line))
                .collect();
            format!(
                "{category} {} (elsewhere: {})",
                nodes.len(),
                elsewhere.join(" ")
            )
        })
        .collect();
    eprintln!(
        "scanned {} nodes in {opened} programs ({} files skipped: {skipped:?}); missed: {}",
        found.len(),
        skipped.len(),
        by_category.join(", ")
    );
}

#[test]
fn the_list_has_no_stale_entry() {
    let (found, ..) = sweep_everything();
    let hit = missed(&found);
    let stale: Vec<&str> = KNOWN_GAPS
        .iter()
        .map(|(category, ..)| *category)
        .filter(|category| !hit.contains_key(category))
        .collect();
    assert!(
        stale.is_empty(),
        "on the list, and no fixture hits them: {stale:?}"
    );
    // and the list names nothing that is a bug
    for bug in [
        TOP_CLASS_DEFINITION,
        TOP_CLASS_COMPUTED_NAME,
        NESTED_CLASS_COMPUTED_NAME,
        INSTANCE_INITIALIZER,
        IN_NAMED_FUNCTION,
        OTHER,
    ] {
        assert!(
            KNOWN_GAPS.iter().all(|(category, ..)| *category != bug),
            "{bug} is a bug, not a gap"
        );
    }
}

/// The counters and the spans are one answer: a class taken as an entry notes, in `Stats.language`, exactly the sites its construction reports as spans, and
/// a body notes the decorators of the classes it declares. Without it the spans could say a site is counted that no counter counts.
#[test]
fn the_counters_say_what_the_spans_say() {
    use analyzer_core::graph::Entry;

    let root = fixtures().join("coverage");
    let project = crate::Project::open(&root, Path::new("src/top-class.ts")).unwrap();
    let sem = &project.sem;
    let counter =
        |graph: &Graph, key: &str| graph.stats.language.get(key).copied().unwrap_or(0) as usize;
    let mut classes_with_sites = 0;
    for func in sem.project_defs(STYLE) {
        let info = sem.describe(func, STYLE);
        let entry = Entry {
            qualname: &info.qualname,
            occurrence: info.occurrence,
        };
        let graph = crate::calls::call_graph(
            &project,
            &info.file,
            &entry,
            &crate::calls::Options::new(STYLE),
            &Cancel::new(),
        )
        .unwrap();
        if sem.def(func, STYLE).kind == DefKind::Class {
            let (_, chain) = sem.init_chain(func, STYLE);
            let initializers: usize = chain
                .iter()
                .map(|c| sem.initializer_sites(*c, STYLE).len())
                .sum();
            let spans = [
                ("uncovered_construct_initializer_sites", initializers),
                (
                    "uncovered_class_definition_sites",
                    sem.definition_sites(func, STYLE).len(),
                ),
                (
                    "uncovered_computed_member_name_sites",
                    sem.computed_name_sites(func, STYLE).len(),
                ),
            ];
            for (key, want) in spans {
                assert_eq!(counter(&graph, key), want, "{key} of {}", info.qualname);
            }
            classes_with_sites += usize::from(spans.iter().any(|(_, n)| *n > 0));
        } else if let Some(body) = sem.body(func, STYLE) {
            let defs = sem.file_defs(func.file, STYLE).unwrap();
            let decorators: usize = body
                .nested
                .iter()
                .filter(|(def, _)| defs.outline.defs[*def].kind == DefKind::Class)
                .map(|&(def, _)| {
                    sem.decorator_sites(
                        Func {
                            file: func.file,
                            def,
                        },
                        STYLE,
                    )
                    .len()
                })
                .sum();
            // a function that constructs a local class counts its decorators once, as a construction counts the class's other sites
            assert_eq!(
                counter(&graph, "uncovered_local_class_decorator_sites"),
                decorators,
                "decorators of {}",
                info.qualname
            );
        }
    }
    assert!(
        classes_with_sites >= 3,
        "{classes_with_sites} classes with sites: the check is vacuous"
    );
}

/// How a program reads, as the owner will use it: what was scanned, what became of it, the misses by category with examples.
fn report(label: &str, analysis: &Analysis, examples: usize) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    let found = &analysis.found;
    let count = |wanted: fn(&Outcome) -> bool| found.iter().filter(|n| wanted(&n.outcome)).count();
    let _ = writeln!(
        out,
        "coverage of {label}: {} files, {} call-like nodes scanned",
        analysis.files.len(),
        found.len()
    );
    let mut excluded: BTreeMap<Policy, usize> = BTreeMap::new();
    let mut counted: BTreeMap<&str, usize> = BTreeMap::new();
    for node in found {
        match node.outcome {
            Outcome::Excluded(policy) => *excluded.entry(policy).or_default() += 1,
            Outcome::Counted(counter) => *counted.entry(counter).or_default() += 1,
            _ => {}
        }
    }
    let _ = writeln!(
        out,
        "  excluded by policy {}: {excluded:?}",
        count(|o| matches!(o, Outcome::Excluded(_)))
    );
    let _ = writeln!(out, "  drawn {}", count(|o| matches!(o, Outcome::Drawn)));
    let _ = writeln!(
        out,
        "  counted {}: {counted:?}",
        count(|o| matches!(o, Outcome::Counted(_)))
    );
    let _ = writeln!(
        out,
        "  missed {}",
        count(|o| matches!(o, Outcome::Missed(_)))
    );
    for (category, nodes) in missed(found) {
        let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
        for node in &nodes {
            *kinds.entry(node.what).or_default() += 1;
        }
        let _ = writeln!(out, "    {category}: {} {kinds:?}", nodes.len());
        for node in nodes.iter().take(examples) {
            let _ = writeln!(
                out,
                "      {}:{}  {}  `{}`",
                node.file, node.line, node.what, node.text
            );
        }
    }
    let _ = writeln!(
        out,
        "  drawn spans the scan does not have, by edge kind: {:?}",
        analysis.drawn_not_scanned
    );
    let _ = writeln!(
        out,
        "  anchors that matched by start only {}, anchors shared by two nodes {}, cuts {}",
        analysis.start_only, analysis.shared_anchors, analysis.cuts
    );
    out
}

/// A measurement on a real project, not a check; it asserts nothing. `TS_MEASURE_ROOT=<dir> TS_MEASURE_FILE=<a file of it> [TS_MEASURE_EXAMPLES=<n>]
/// cargo test --release -p typescript-analysis --lib measure_coverage -- --ignored --nocapture`. Every def of the project is an entry, so "missed" means
/// "drawn in the tree of no def of the project and counted by no counter". The owner decides each category from these numbers.
#[test]
#[ignore = "a measurement on a real project, driven by TS_MEASURE_ROOT and TS_MEASURE_FILE"]
fn measure_coverage() {
    let (Ok(root), Ok(file)) = (
        std::env::var("TS_MEASURE_ROOT"),
        std::env::var("TS_MEASURE_FILE"),
    ) else {
        eprintln!("set TS_MEASURE_ROOT and TS_MEASURE_FILE");
        return;
    };
    let examples = std::env::var("TS_MEASURE_EXAMPLES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(5);
    let sem = Sem::open(Path::new(&root), Path::new(&file)).expect("the project opens");
    let started = std::time::Instant::now();
    let analysis = analyze(&sem);
    eprintln!("scan and graph: {:?}", started.elapsed());
    eprint!("{}", report(&root, &analysis, examples));
}
