//! The body of a def as owned data (`docs/08-bend/07-value-layer-design.md` §4): what the item list cannot say, because a list has no `let`, no `match` and no body of
//! a lambda. It is built by the same pass that builds the items (`walk.rs`), so the two cannot disagree about where a site is: **every node that is a site carries the
//! index of its `Item`** in `DefWalk::items`, and the evaluator decides the targets of items, never a site of its own.
//!
//! A binder (a parameter, a `let` or `case` name, a lambda's binder, a do's name, a rewrite's equation, an `All` binder) has a number, per def: the parameters are
//! `0..params` in written order, every other binder takes the next number when it is bound. A name that a binder binds is `Var(number)`; any other name is `Name`.
//!
//! No `bend_parser` type is here (rule 4): this is data the rest of the crate may read.

use std::fmt;

/// One def's body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Flow {
    /// Binders `0..params` are the def's parameters, in written order.
    pub(crate) params: u32,
    /// By `nth`, the number the walk gave each lambda (`ArgShape::Lambda`, `Known::Lambda`): a chain `w => b => body` has two numbers, and the second is the lambda
    /// `b => body` that applying the first to one argument gives.
    pub(crate) lambdas: Box<[LamFacts]>,
    pub(crate) body: Expr,
}

/// What one lambda says of itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LamFacts {
    /// How many binders its chain has from here: `w => b => body` is 2 for the lambda `w => ..` and 1 for the lambda `b => ..` inside it.
    pub(crate) binders: u32,
    /// The binders bound outside the lambda that its body reads, nested lambdas' bodies included: sorted, no duplicates.
    pub(crate) reads: Box<[u32]>,
}

/// A term, as far as the layer reads it. `item` is an index into the items of the same def.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Expr {
    /// A term the layer does not model, or whose value it cannot know (a list, a pair, a hole): the value of an unknown is open.
    Unknown,
    /// A literal, an operator on Base, a type: nothing callable is there.
    Data,
    /// A binder of this def, by number.
    Var(u32),
    /// A name that no binder binds. `item` is its `Passed` or `Deep` place; the callee of a call has none (the call's own item is where it is).
    Name { item: Option<u32>, name: String },
    /// `x => body`, `w => b => body` (one node, two binders), `\{ .. }` (one binder, no name).
    Lam {
        nth: u32,
        binders: Box<[u32]>,
        body: Box<Expr>,
    },
    /// `f(a, b)`: one node with two arguments. The item is a `Call` or an `Opaque`, whichever the callee made it.
    Call {
        item: u32,
        head: Box<Expr>,
        args: Box<[Expr]>,
    },
    /// `K{a, b}`.
    Ctr {
        item: u32,
        name: String,
        fields: Box<[Expr]>,
    },
    /// An operator, an index or an array that stands for a call, with its operands as written.
    Sugar { item: u32, operands: Box<[Expr]> },
    /// A `do` statement of the monad `monad`, as `bend.ts` makes it a call (`parse_term_do_stmt`): `x : A <- v` in `do M<T.., B>:` is
    /// `M.bind(T.., A, B, v, x => rest)` and `return e` is `M.pure(T.., B, e)`. `types` are the type terms in that order (`T.., A, B` for a bind; `T.., B` for a pure),
    /// all `Data`. `binder` is the continuation's binder (a statement with no name has a number nobody reads); a `pure` has none and its `rest` is never read.
    Bind {
        item: u32,
        monad: String,
        types: Box<[Expr]>,
        value: Box<Expr>,
        binder: Option<u32>,
        rest: Box<Expr>,
        pure: bool,
    },
    /// `p = v` and the rest: the values are read before the patterns bind.
    Let {
        pats: Box<[Pat]>,
        values: Box<[Expr]>,
        rest: Box<Expr>,
    },
    /// `match a b: case p q: body`: one pattern per scrutinee in every case.
    Match {
        on: Box<[Expr]>,
        cases: Box<[(Box<[Pat]>, Expr)]>,
    },
    /// Terms read for their sites; the value is the last one's.
    Seq(Box<[Expr]>),
}

/// What a pattern binds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pat {
    /// A name.
    Bind(u32),
    /// `K{p, q}`.
    Ctr { name: String, fields: Box<[Pat]> },
    /// A list, a tuple, `3n+p`, `h <> t`: the binders inside, which the layer does not follow.
    Opaque(Box<[u32]>),
    /// A literal, or anything that binds nothing.
    Skip,
}

/// What kind of node carries an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    Call,
    Ctr,
    Sugar,
    Bind,
    Name,
}

impl Expr {
    /// Every item this node and the nodes under it carry, with the kind of node that carries it, in the order the tree is read.
    pub(crate) fn items(&self, out: &mut Vec<(u32, Node)>) {
        match self {
            Expr::Unknown | Expr::Data | Expr::Var(_) => {}
            Expr::Name { item, .. } => out.extend(item.map(|i| (i, Node::Name))),
            Expr::Lam { body, .. } => body.items(out),
            Expr::Call { item, head, args } => {
                out.push((*item, Node::Call));
                head.items(out);
                args.iter().for_each(|a| a.items(out));
            }
            Expr::Ctr { item, fields, .. } => {
                out.push((*item, Node::Ctr));
                fields.iter().for_each(|f| f.items(out));
            }
            Expr::Sugar { item, operands } => {
                out.push((*item, Node::Sugar));
                operands.iter().for_each(|o| o.items(out));
            }
            Expr::Bind {
                item,
                types,
                value,
                rest,
                ..
            } => {
                out.push((*item, Node::Bind));
                types.iter().for_each(|t| t.items(out));
                value.items(out);
                rest.items(out);
            }
            Expr::Let { values, rest, .. } => {
                values.iter().for_each(|v| v.items(out));
                rest.items(out);
            }
            Expr::Match { on, cases } => {
                on.iter().for_each(|e| e.items(out));
                cases.iter().for_each(|(_, body)| body.items(out));
            }
            Expr::Seq(parts) => parts.iter().for_each(|p| p.items(out)),
        }
    }

    /// Every node of the tree, this one first, then each part in the order the tree is read.
    pub(crate) fn walk(&self, f: &mut dyn FnMut(&Expr)) {
        f(self);
        match self {
            Expr::Unknown | Expr::Data | Expr::Var(_) | Expr::Name { .. } => {}
            Expr::Lam { body, .. } => body.walk(f),
            Expr::Call { head, args, .. } => {
                head.walk(f);
                args.iter().for_each(|a| a.walk(f));
            }
            Expr::Ctr { fields, .. } => fields.iter().for_each(|e| e.walk(f)),
            Expr::Sugar { operands, .. } => operands.iter().for_each(|e| e.walk(f)),
            Expr::Bind {
                types, value, rest, ..
            } => {
                types.iter().for_each(|t| t.walk(f));
                value.walk(f);
                rest.walk(f);
            }
            Expr::Let { values, rest, .. } => {
                values.iter().for_each(|v| v.walk(f));
                rest.walk(f);
            }
            Expr::Match { on, cases } => {
                on.iter().for_each(|e| e.walk(f));
                cases.iter().for_each(|(_, body)| body.walk(f));
            }
            Expr::Seq(parts) => parts.iter().for_each(|p| p.walk(f)),
        }
    }

    /// The same tree with every item index `f` of it.
    pub(crate) fn map_items(&mut self, f: &dyn Fn(u32) -> u32) {
        match self {
            Expr::Unknown | Expr::Data | Expr::Var(_) => {}
            Expr::Name { item, .. } => *item = item.map(f),
            Expr::Lam { body, .. } => body.map_items(f),
            Expr::Call { item, head, args } => {
                *item = f(*item);
                head.map_items(f);
                args.iter_mut().for_each(|a| a.map_items(f));
            }
            Expr::Ctr { item, fields, .. } => {
                *item = f(*item);
                fields.iter_mut().for_each(|e| e.map_items(f));
            }
            Expr::Sugar { item, operands } => {
                *item = f(*item);
                operands.iter_mut().for_each(|o| o.map_items(f));
            }
            Expr::Bind {
                item,
                types,
                value,
                rest,
                ..
            } => {
                *item = f(*item);
                types.iter_mut().for_each(|t| t.map_items(f));
                value.map_items(f);
                rest.map_items(f);
            }
            Expr::Let { values, rest, .. } => {
                values.iter_mut().for_each(|v| v.map_items(f));
                rest.map_items(f);
            }
            Expr::Match { on, cases } => {
                on.iter_mut().for_each(|e| e.map_items(f));
                cases.iter_mut().for_each(|(_, body)| body.map_items(f));
            }
            Expr::Seq(parts) => parts.iter_mut().for_each(|p| p.map_items(f)),
        }
    }
}

impl Flow {
    /// Every item the body carries and the node that carries it, sorted by item: what the test that each site is one node reads.
    pub(crate) fn items(&self) -> Vec<(u32, Node)> {
        let mut out = Vec::new();
        self.body.items(&mut out);
        out.sort_by_key(|(item, _)| *item);
        out
    }

    /// What the lambdas say, one per `nth`: `0: 2 binders, reads [0 1]`.
    pub(crate) fn facts_text(&self) -> String {
        let lams: Vec<String> = self
            .lambdas
            .iter()
            .enumerate()
            .map(|(nth, l)| {
                let reads: Vec<String> = l.reads.iter().map(|r| format!("${r}")).collect();
                format!("{nth}: {} binders, reads [{}]", l.binders, reads.join(" "))
            })
            .collect();
        format!("params {}; {}", self.params, lams.join("; "))
    }
}

/// The text form, for the unit tests: one line, `$n` a binder, `name#i` a name with its item, `(call#i head args..)`, `(lam nth [binders] body)`.
impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Unknown => f.write_str("?"),
            Expr::Data => f.write_str("data"),
            Expr::Var(n) => write!(f, "${n}"),
            Expr::Name { item: None, name } => f.write_str(name),
            Expr::Name {
                item: Some(item),
                name,
            } => write!(f, "{name}#{item}"),
            Expr::Lam { nth, binders, body } => {
                write!(
                    f,
                    "(lam {nth} [{}] {body})",
                    list(binders, |b| format!("${b}"))
                )
            }
            Expr::Call { item, head, args } => {
                write!(f, "(call#{item} {head}{})", each(args))
            }
            Expr::Ctr { item, name, fields } => {
                write!(f, "(ctr#{item} {name}{})", each(fields))
            }
            Expr::Sugar { item, operands } => write!(f, "(sugar#{item}{})", each(operands)),
            Expr::Bind {
                item,
                monad,
                types,
                value,
                binder,
                rest,
                pure,
            } => {
                let types = list(types, |t| t.to_string());
                if *pure {
                    write!(f, "(pure#{item} {monad} [{types}] {value})")
                } else {
                    let binder = binder.map_or_else(|| "_".to_owned(), |b| format!("${b}"));
                    write!(f, "(bind#{item} {monad} [{types}] {value} {binder} {rest})")
                }
            }
            Expr::Let { pats, values, rest } => write!(
                f,
                "(let [{}] [{}] {rest})",
                list(pats, |p| p.to_string()),
                list(values, |v| v.to_string())
            ),
            Expr::Match { on, cases } => {
                write!(f, "(match [{}]", list(on, |e| e.to_string()))?;
                for (pats, body) in cases.iter() {
                    write!(f, " (case [{}] {body})", list(pats, |p| p.to_string()))?;
                }
                f.write_str(")")
            }
            Expr::Seq(parts) => write!(f, "(seq{})", each(parts)),
        }
    }
}

impl fmt::Display for Pat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Pat::Bind(n) => write!(f, "${n}"),
            Pat::Ctr { name, fields } => {
                write!(f, "{name}{{{}}}", list(fields, |p| p.to_string()))
            }
            Pat::Opaque(binders) => write!(f, "~[{}]", list(binders, |b| format!("${b}"))),
            Pat::Skip => f.write_str("_"),
        }
    }
}

fn list<T>(items: &[T], one: impl Fn(&T) -> String) -> String {
    items.iter().map(one).collect::<Vec<_>>().join(" ")
}

/// Each expression after a space.
fn each(items: &[Expr]) -> String {
    items.iter().map(|e| format!(" {e}")).collect()
}

#[cfg(test)]
mod tests {
    //! The text form of the `Flow` of one def, per node kind. Each expected text was written from the rules of `walk.rs` before the code was run: items are numbered in
    //! source order (by start, then end, parameters' types first), binders are numbered parameters first and then in the order they are bound.

    use std::fs;
    use std::path::{Path, PathBuf};

    use super::super::walk::{What, walks};
    use super::*;

    /// The flow of the one def of `text`.
    fn flow(text: &str) -> Flow {
        let walks = walks(text);
        assert_eq!(walks.len(), 1, "one declaration: {text}");
        walks[0].flow.clone()
    }

    fn body(text: &str) -> String {
        flow(text).body.to_string()
    }

    #[test]
    fn a_call_through_a_name_and_one_through_a_parameter() {
        // items: the type of `x`, the type of the reply, the call
        assert_eq!(body("def g(x: U32) -> U32:\n  f(x)\n"), "(call#2 f $0)");
        // `f` is a parameter: the callee is a read of the binder
        assert_eq!(
            body("def g(f: U32 -> U32, x: U32) -> U32:\n  f(x)\n"),
            "(call#3 $0 $1)"
        );
        assert_eq!(
            flow("def g(f: U32 -> U32, x: U32) -> U32:\n  f(x)\n").params,
            2
        );
    }

    #[test]
    fn a_name_handed_over_and_a_name_below_the_arguments() {
        // the reply type is item 0, the call 1, the name handed over 2; a literal has no item
        assert_eq!(
            body("def g() -> U32:\n  each(work, 1)\n"),
            "(call#1 each work#2 data)"
        );
        // a bare name that is the reply is `Deep`
        assert_eq!(body("def g() -> U32:\n  work\n"), "work#1");
        // a group with no `: T` is the term inside it
        assert_eq!(
            body("def g() -> U32:\n  each((work), 1)\n"),
            "(call#1 each work#2 data)"
        );
    }

    #[test]
    fn a_lambda_and_what_it_reads_of_its_writer() {
        let text = "def g(h: U32 -> U32, x: U32) -> U32:\n  each(y => h(y), x)\n";
        // items: types 0 to 2, `each` 3, `h(y)` 4; `y` is the third binder
        assert_eq!(body(text), "(call#3 each (lam 0 [$2] (call#4 $0 $2)) $1)");
        assert_eq!(
            flow(text).facts_text(),
            "params 2; 0: 1 binders, reads [$0]"
        );
    }

    #[test]
    fn a_chain_of_lambdas_is_one_node_with_the_binders_of_both() {
        let text = "def g() -> U32:\n  each(a => b => work(a), 1)\n";
        assert_eq!(
            body(text),
            "(call#1 each (lam 0 [$0 $1] (call#2 work $0)) data)"
        );
        // the second `=>` is a lambda of its own to the walk (number 1): the one `a` applied to one argument gives, which reads `a`
        assert_eq!(
            flow(text).facts_text(),
            "params 0; 0: 2 binders, reads []; 1: 1 binders, reads [$0]"
        );
    }

    #[test]
    fn a_lambda_reads_what_a_lambda_inside_it_reads() {
        let text = "def g(h: U32 -> U32) -> U32:\n  each(x => each(y => h(x), 1), 2)\n";
        assert_eq!(
            body(text),
            "(call#2 each (lam 0 [$1] (call#3 each (lam 1 [$2] (call#4 $0 $1)) data)) data)"
        );
        assert_eq!(
            flow(text).facts_text(),
            "params 1; 0: 1 binders, reads [$0]; 1: 1 binders, reads [$0 $1]"
        );
    }

    #[test]
    fn a_match_lambda_has_one_binder_with_no_name_and_an_unknown_answer() {
        let text = "def g() -> U32:\n  k(x => m(y => y), z => z, \\{ Nil: 1 })\n";
        assert_eq!(
            body(text),
            "(call#1 k (lam 0 [$0] (call#2 m (lam 1 [$1] $1))) (lam 2 [$2] $2) (lam 3 [$3] (seq data ?)))"
        );
    }

    #[test]
    fn a_let_reads_its_values_before_its_names_bind() {
        // `y = f(x)`: `f(x)` is item 2, and the reply `y` is the binder after the parameter
        assert_eq!(
            body("def g(x: U32) -> U32:\n  y = f(x)\n  y\n"),
            "(let [$1] [(call#2 f $0)] $1)"
        );
        // `f = f(x)`: the call is on the name, and the next `f(1)` is on the binder
        assert_eq!(
            body("def g(x: U32) -> U32:\n  f = f(x)\n  f(1)\n"),
            "(let [$1] [(call#2 f $0)] (call#3 $1 data))"
        );
    }

    #[test]
    fn the_items_are_sorted_by_place_and_the_nodes_follow() {
        // the value is walked before the written type, which comes first in the text: the call is item 2 and the type of `x` is item 1
        assert_eq!(
            body("def g() -> U32:\n  x : U32 = f(1)\n  x\n"),
            "(let [$0] [(call#2 f data)] $0)"
        );
        // two places at one span keep the order they were taken in, the outer first: the call 3 on the sugar 4 that it stands on
        assert_eq!(
            body("def g(a: Arr, i: U32) -> U32:\n  a[i](1)\n"),
            "(call#3 (sugar#4 $0 $1) data)"
        );
    }

    #[test]
    fn a_match_binds_its_patterns_in_its_cases() {
        let text = "def g(xs: List) -> U32:\n  match xs:\n    case Cons{h, t}:\n      h(1)\n    case Nil{}:\n      2\n";
        assert_eq!(
            body(text),
            "(match [$0] (case [Cons{$1 $2}] (call#2 $1 data)) (case [Nil{}] data))"
        );
        // a pattern the layer does not follow binds its names to nothing it can say
        assert_eq!(
            body("def g(xs: List) -> U32:\n  match xs:\n    case h <> t:\n      h(1)\n"),
            "(match [$0] (case [~[$1 $2]] (call#2 $1 data)))"
        );
        assert_eq!(
            body("def g(n: U32) -> U32:\n  match n:\n    case 0:\n      1\n    case m:\n      2\n"),
            "(match [$0] (case [_] data) (case [$1] data))"
        );
    }

    #[test]
    fn a_constructor_holds_its_fields_and_a_sugar_its_operands() {
        // the reply type 0, the constructor 1, the name handed over 2
        assert_eq!(
            body("def g() -> Shape:\n  Circle{work, 1}\n"),
            "(ctr#1 Circle work#2 data)"
        );
        // the operator is item 3 (after the three types); the `: U32` of the group is item 4
        assert_eq!(
            body("def g(a: U32, b: U32) -> U32:\n  (a + b : U32)\n"),
            "(sugar#3 $0 $1)"
        );
        assert_eq!(
            body("def g(a: Arr, i: U32) -> U32:\n  a[i]\n"),
            "(sugar#3 $0 $1)"
        );
        // a write has its value as the third operand
        assert_eq!(
            body("def g(a: Arr, i: U32) -> Arr:\n  a[i] <- 7\n"),
            "(sugar#3 $0 $1 data)"
        );
    }

    #[test]
    fn a_do_statement_is_a_node_that_holds_the_rest() {
        // items: the type of the reply 0, `U32` of `IO<U32>` 1; then `x : U32 <- a()`: the sugar 2, its type 3, `a()` 4; `b(x)`: the call 5 and the sugar 6 (one
        // span starts at the same place, the call's ends first); `return c(x)`: the sugar 7, the call 8. `x` is the first binder, the step's `_` the second
        assert_eq!(
            body(
                "def g() -> IO(U32):\n  do IO<U32>:\n    x : U32 <- a()\n    b(x)\n    return c(x)\n"
            ),
            "(bind#2 IO [data data] (call#4 a) $0 (bind#6 IO [data data] (call#5 b $0) $1 (pure#7 IO [data] (call#8 c $0))))"
        );
    }

    #[test]
    fn a_do_let_is_a_let_and_the_final_value_is_the_value() {
        // `x : U32 = f()` is a let in the do: items are the reply type 0, `U32` of `IO<U32>` 1, the let's type 2, the call 3 (no sugar: nothing is bound by a call); the
        // final value stands for itself
        assert_eq!(
            body("def g() -> U32:\n  do IO<U32>:\n    x : U32 = f()\n    x\n"),
            "(let [$0] [(call#3 f)] $0)"
        );
    }

    #[test]
    fn what_the_layer_does_not_model_is_unknown_and_what_cannot_be_called_is_data() {
        // a list holds what it holds: unknown, after its items
        assert_eq!(
            body("def g() -> U32:\n  [work, 1]\n"),
            "(seq work#1 data ?)"
        );
        // a literal and a type
        assert_eq!(body("def g() -> U32:\n  7\n"), "data");
        assert_eq!(body("def g() -> Type:\n  U32\n"), "U32#1");
    }

    #[test]
    fn a_def_with_no_code_has_an_unknown_body_and_no_lambdas() {
        let walks = walks("law l:\n  for x: U32\n  {x == x : U32}\n");
        assert_eq!(walks[0].flow.body, Expr::Unknown);
        assert_eq!(walks[0].flow.params, 0);
        assert!(walks[0].flow.lambdas.is_empty());
    }

    /// Every `.bend` file under `dir`, in a stable order.
    fn bend_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<PathBuf> = fs::read_dir(dir)
            .expect("a folder")
            .map(|e| e.expect("an entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                bend_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "bend") {
                out.push(path);
            }
        }
    }

    /// The proof of §4: each item that is a site (and each `Deep` name) is carried by exactly one node of its def's flow, by a node of the kind that makes
    /// that item, and no node carries a place that is not a site. Returns how many sites were checked.
    fn each_site_is_one_node(text: &str, whose: &str) -> usize {
        let mut checked = 0;
        for walk in walks(text) {
            let carried = walk.flow.items();
            let expected: Vec<(u32, Node)> = walk
                .items
                .iter()
                .enumerate()
                .filter_map(|(at, item)| {
                    let node = match &item.what {
                        What::Call { .. } | What::Opaque { .. } => Node::Call,
                        What::Ctr { .. } => Node::Ctr,
                        What::Sugar(_) => {
                            // a sugar is carried by an operator, an index, an array, or a do statement: the node kind is whichever carries it
                            return carried.iter().find(|(c, _)| *c == at as u32).copied();
                        }
                        What::Passed { .. } | What::Deep { .. } => Node::Name,
                        What::BaseFixed | What::Data | What::TypePosition => return None,
                    };
                    Some((at as u32, node))
                })
                .collect();
            assert_eq!(
                carried, expected,
                "{whose}: the def at {:?}",
                walk.name_span
            );
            checked += expected.len();
        }
        checked
    }

    #[test]
    fn every_site_of_every_fixture_and_scenario_is_one_node() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        bend_files(&manifest.join("tests/fixtures"), &mut files);
        bend_files(&manifest.join("../../../../scenarios"), &mut files);
        assert!(files.len() > 60, "the fixtures were found: {}", files.len());
        let checked: usize = files
            .iter()
            .map(|file| {
                let text = fs::read_to_string(file).expect("a file");
                each_site_is_one_node(&text, &file.display().to_string())
            })
            .sum();
        assert!(checked > 200, "sites were checked: {checked}");
    }

    /// The same over the demos' books and every file of the corpus: `BEND_CLONE=.. cargo test -p bend-analysis --lib -- --ignored`.
    #[test]
    #[ignore = "needs BEND_CLONE"]
    fn every_site_of_the_demos_is_one_node() {
        let given = std::env::var_os("BEND_CLONE").expect("BEND_CLONE names the bend clone");
        let clone = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../..")
            .join(given);
        let mut files = Vec::new();
        bend_files(&clone.join("demos"), &mut files);
        assert!(!files.is_empty(), "the demos were found");
        let checked: usize = files
            .iter()
            .map(|file| {
                let text = fs::read_to_string(file).expect("a file");
                each_site_is_one_node(&text, &file.display().to_string())
            })
            .sum();
        println!("{} files, {checked} sites, each one node", files.len());
    }
}
