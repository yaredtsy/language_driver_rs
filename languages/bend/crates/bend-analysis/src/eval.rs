//! Evaluating a body (`docs/08-bend/07-value-layer-design.md` §5): under `Precision::Precise` a def is walked by reading its `Flow` under its key, and the answer is,
//! for each item of the def, what it comes to (`Reached`), the same type the item walk of layer 1 gives. The evaluator decides the **targets** of items and never
//! invents a site: every site is an item (BV-D2), which is what makes "it knows nothing, so it is layer 1" a test that can be run item by item.
//!
//! This step tracks exactly what BC1+BC2 tracked, and nothing more: a parameter has the value the key gives it, a def named in an argument is that def, a lambda is a
//! lambda that **stays** (its body is read here, where it is written, its binders unknown), and nothing else carries a value. No variable has one, no call has a
//! return value, no constructor has fields. The steps that follow (V3 onwards) give each of those a value, one at a time, with the rows that flip.
//!
//! 🦀 The evaluator is a struct that borrows the book, the key's environment and the def's tree, and owns the one thing it fills in: a `Vec<Option<Reached>>`, one
//! place for each item. `Option` because an item that is not a node (a type skipped, Base's `&&`) is not the evaluator's: the walk that calls it settles those.

use analyzer_core::graph::Reason;
use analyzer_core::solve::Target;

use crate::book::{DeclId, Resolved};
use crate::calls::{self, Bend};
use crate::env::Env;
use crate::key::{self, Absent, Key, Known};
use crate::sem::{ArgShape, DefWalk, Expr, Item, What};
use crate::value::{One, Value};
use crate::walk::{self, Reached};

/// What the evaluator may know. `Tracked` is the layer. `Nothing` is for the test that proves the evaluator and the item table agree: every value is unknown, so
/// the answer must be layer 1's, site by site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Policy {
    Tracked,
    #[cfg(test)]
    Nothing,
}

struct Eval<'l, 'b> {
    lang: &'l Bend<'b>,
    def: DeclId,
    env: &'l Env<'l>,
    walk: &'b DefWalk,
    /// One place for each item of the def, filled in by the node that carries it.
    reached: Vec<Option<Reached<'b>>>,
}

/// What each item of `f` comes to under `env`, by item: `Some` for every item that is a site and for every name (handed over, or below the arguments), `None` for the
/// rest, which the item walk settles. Empty when `f` has no walk (not a def of the book).
pub(crate) fn reached<'b>(lang: &Bend<'b>, f: DeclId, env: &Env<'_>) -> Vec<Option<Reached<'b>>> {
    let Some(walk) = lang.walk_of(f) else {
        return Vec::new();
    };
    let mut eval = Eval {
        lang,
        def: f,
        env,
        walk,
        reached: (0..walk.items.len()).map(|_| None).collect(),
    };
    eval.eval(&walk.flow.body);
    eval.reached
}

impl<'b> Eval<'_, 'b> {
    /// What a value of the layer is, as far as the policy lets it be known.
    fn known(&self, value: Value) -> Value {
        match self.lang.policy {
            Policy::Tracked => value,
            #[cfg(test)]
            Policy::Nothing => Value::unknown(),
        }
    }

    fn item(&self, at: u32) -> &'b Item {
        &self.walk.items[at as usize]
    }

    /// What a bare name stands for: a def with code is that def, a type or a law alone is data, and Base's, a hub's and what the book does not know are unknown.
    fn name_value(&self, name: &str) -> Value {
        let value = match self.lang.book.resolve(self.def.file(), name) {
            Resolved::Project(id) if key::known_def(self.lang, id) => Value::one(One::Def(id)),
            Resolved::Project(_) => Value::data(),
            _ => Value::unknown(),
        };
        self.known(value)
    }

    fn eval(&mut self, expr: &Expr) -> Value {
        match expr {
            Expr::Unknown => Value::unknown(),
            Expr::Data => Value::data(),
            // a parameter has what the key says; a binder of any other kind has nothing yet
            Expr::Var(n) if *n < self.walk.flow.params => {
                let value = self
                    .env
                    .known(*n)
                    .map_or_else(Value::unknown, Value::from_known);
                self.known(value)
            }
            Expr::Var(_) => Value::unknown(),
            Expr::Name { item, name } => {
                if let Some(at) = item {
                    let reached =
                        walk::resolve_from_book(self.lang, self.def, self.item(*at), self.env);
                    self.reached[*at as usize] = Some(reached);
                }
                self.name_value(name)
            }
            // a lambda stays with its writer: its body is read here, with its binders unknown, and it is the value of the lambda `nth` of this def
            Expr::Lam { nth, body, .. } => {
                self.eval(body);
                self.known(Value::one(One::Lambda {
                    writer: self.def,
                    nth: *nth,
                }))
            }
            Expr::Call { item, head, args } => {
                // a callee that is a name has no place of its own: it is the call's
                let head_value = match &**head {
                    Expr::Name { item: None, name } => self.name_value(name),
                    other => self.eval(other),
                };
                let values: Vec<Value> = args.iter().map(|a| self.eval(a)).collect();
                self.reached[*item as usize] = Some(self.call(*item, &head_value, &values));
                Value::unknown()
            }
            Expr::Ctr { item, fields, .. } => {
                fields.iter().for_each(|f| drop(self.eval(f)));
                self.settle(*item);
                Value::unknown()
            }
            Expr::Sugar { item, operands } => {
                operands.iter().for_each(|o| drop(self.eval(o)));
                self.settle(*item);
                Value::data()
            }
            // a statement of a `do` is a call to the monad's `bind` or `pure`; its continuation stays, read here
            Expr::Bind {
                item, value, rest, ..
            } => {
                self.eval(value);
                self.eval(rest);
                self.settle(*item);
                Value::unknown()
            }
            Expr::Let { values, rest, .. } => {
                values.iter().for_each(|v| drop(self.eval(v)));
                self.eval(rest)
            }
            // no condition is evaluated and no case is ruled out: the value is the join of the cases
            Expr::Match { on, cases } => {
                on.iter().for_each(|e| drop(self.eval(e)));
                cases
                    .iter()
                    .map(|(_, body)| self.eval(body))
                    .reduce(|a, b| a.join(&b))
                    .unwrap_or_else(Value::data)
            }
            Expr::Seq(parts) => parts
                .iter()
                .map(|p| self.eval(p))
                .last()
                .unwrap_or_else(Value::data),
        }
    }

    /// An item that no value changes the answer for: the book's, as layer 1 gives it.
    fn settle(&mut self, at: u32) {
        let reached = walk::resolve_from_book(self.lang, self.def, self.item(at), self.env);
        self.reached[at as usize] = Some(reached);
    }

    /// What a call comes to: its callee is a name the book resolves, or a value. `head` is the value of the callee, `values` those of the arguments.
    fn call(&self, at: u32, head: &Value, values: &[Value]) -> Reached<'b> {
        let item = self.item(at);
        match &item.what {
            // an unbound name: the book fixes what it is (a name it fixes is never overridden), and the key is made of the arguments
            What::Call { name, args } => {
                let resolved = self.lang.book.resolve(self.def.file(), name);
                let key = match &resolved {
                    Resolved::Project(id) => self.key(*id, args, values),
                    _ => Key::open(),
                };
                walk::site(walk::call_of(self.lang, resolved, name, key), false)
            }
            // a call through the def's own parameter: what the key says it is. BC1+BC2 follows nothing else: a bound name that is not a parameter, and a callee that
            // is not a name (`f(a)(b)`, `(x => ..)(a)`), are values the layer does not carry yet
            What::Opaque {
                param: Some(_),
                args,
            } => match head.single() {
                Some(One::Def(callee)) => walk::site(
                    walk::project(self.lang, callee, self.key(callee, args, values)),
                    false,
                ),
                Some(One::Lambda { .. }) => walk::site(
                    Target::Unresolved(Reason::new(calls::ANONYMOUS_FUNCTION_CALLED)),
                    false,
                ),
                None => walk::site(
                    Target::Unresolved(Reason::new(calls::FUNCTION_TYPED_VALUE)),
                    false,
                ),
            },
            _ => walk::resolve_from_book(self.lang, self.def, item, self.env),
        }
    }

    /// The key of a call of `callee`, from the values of its arguments (06 §4).
    fn key(&self, callee: DeclId, shapes: &[ArgShape], values: &[Value]) -> Key {
        key::key_with(self.lang, callee, |at| {
            self.slot(shapes.get(at as usize), values.get(at as usize))
        })
    }

    /// What the argument at a position puts in a slot. BC1+BC2 read a slot off a bare name or a lambda and off nothing else, so `shape` (what the text says the argument
    /// is) decides whether the value counts; the value says what is in it. A value that is not exactly one def or lambda has no slot, and the shape says why.
    fn slot(&self, shape: Option<&ArgShape>, value: Option<&Value>) -> Result<Known, Absent> {
        let Some(shape) = shape else {
            return Err(Absent::Missing);
        };
        let known = match shape {
            ArgShape::Lambda(_) | ArgShape::Param(_) | ArgShape::Name(_) => {
                value.and_then(Known::of)
            }
            ArgShape::Local | ArgShape::Other => None,
        };
        known.ok_or_else(|| match shape {
            ArgShape::Param(_) => Absent::Open,
            ArgShape::Local => Absent::Local,
            ArgShape::Name(name) => key::absent_name(self.lang, self.def, name),
            ArgShape::Lambda(_) | ArgShape::Other => Absent::Value,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use analyzer_core::edit::MarkerStyle;

    use super::*;
    use crate::book::Book;
    use crate::calls::Precision;

    /// What a `Reached` comes to, as text: the sites and targets compared, in a form the two walks share.
    fn describe(reached: &Reached<'_>) -> String {
        match reached {
            Reached::Quiet => "quiet".to_owned(),
            Reached::Counted(key) => format!("counted {key}"),
            Reached::Site { targets, passed } => {
                let targets: Vec<String> = targets
                    .iter()
                    .map(|target| match target {
                        Target::Call(f, key) => format!("call {f:?} {key:?}"),
                        Target::Construct { class, init } => {
                            format!("construct {class:?} {init:?}")
                        }
                        Target::External(external) => {
                            format!("external {} {}", external.package, external.name)
                        }
                        Target::Unresolved(reason) => format!("unresolved {}", reason.as_str()),
                        _ => unreachable!("a target this test does not know"),
                    })
                    .collect();
                format!("site passed={passed} [{}]", targets.join(", "))
            }
        }
    }

    /// The first difference between the evaluator that knows nothing and layer 1's item walk, over every def of `book`, as text; `None` when there is none.
    /// Returns how many items were compared.
    fn nothing_is_layer_one(book: &Book, whose: &str) -> usize {
        let item_walk = Bend::new(book, MarkerStyle::above("ID"), Precision::Default);
        let mut evaluator = Bend::new(book, MarkerStyle::above("ID"), Precision::Precise);
        evaluator.policy = Policy::Nothing;
        let open = Key::open();
        let mut compared = 0;
        for decl in book.decls() {
            let theirs = walk::reach(&item_walk, decl.id, &Env::new(&open));
            let mut ours = reached(&evaluator, decl.id, &Env::new(&open));
            assert_eq!(theirs.len(), ours.len(), "{whose}: {:?}", decl.id);
            for (at, (item, expected)) in theirs.iter().enumerate() {
                // the item walk settles what the evaluator leaves: an item that is not a node
                let got = ours[at].take().unwrap_or_else(|| {
                    assert!(
                        matches!(item.what, What::TypePosition | What::BaseFixed | What::Data),
                        "{whose}: the item {at} of {:?} is a site and no node settled it: {:?}",
                        decl.id,
                        item.what
                    );
                    walk::resolve_from_book(&evaluator, decl.id, item, &Env::new(&open))
                });
                assert_eq!(
                    describe(&got),
                    describe(expected),
                    "{whose}: item {at} ({:?}) of {:?}",
                    item.what,
                    decl.id
                );
                compared += 1;
            }
        }
        compared
    }

    /// Every folder under `dir` that has a `main.bend`: a book is opened at each, rooted there.
    fn books(dir: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<PathBuf> = fs::read_dir(dir)
            .expect("a folder")
            .map(|e| e.expect("an entry").path())
            .collect();
        entries.sort();
        if entries.iter().any(|p| p.ends_with("main.bend")) {
            out.push(dir.to_path_buf());
        }
        for path in entries.into_iter().filter(|p| p.is_dir()) {
            books(&path, out);
        }
    }

    /// BV-D2, run: with every value unknown the evaluator gives, for every def of every fixture and scenario, what layer 1's item walk gives, item by item: the
    /// sites, their targets, the keys (all open), what is counted.
    #[test]
    fn an_evaluator_that_knows_nothing_is_layer_one() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut folders = Vec::new();
        books(&manifest.join("tests/fixtures"), &mut folders);
        books(&manifest.join("../../../../scenarios"), &mut folders);
        assert!(
            folders.len() > 50,
            "the books were found: {}",
            folders.len()
        );
        let compared: usize = folders
            .iter()
            .map(|folder| {
                let book = Book::open(folder, Path::new("main.bend")).expect("the book opens");
                nothing_is_layer_one(&book, &folder.display().to_string())
            })
            .sum();
        assert!(compared > 500, "items were compared: {compared}");
    }

    /// The same over the 16 demo books of the sweep (each demo's `main.bend`, and `walkers_test.bend` for the netcode, which has none), and over the books that start
    /// at each demo's `PROOF.bend`: `BEND_CLONE=.. cargo test -p bend-analysis --release --lib -- --ignored`.
    #[test]
    #[ignore = "needs BEND_CLONE"]
    fn an_evaluator_that_knows_nothing_is_layer_one_on_the_demos() {
        let given = std::env::var_os("BEND_CLONE").expect("BEND_CLONE names the bend clone");
        let demos = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../..")
            .join(given)
            .join("demos");
        let mut folders: Vec<PathBuf> = fs::read_dir(&demos)
            .expect("the demos")
            .map(|e| e.expect("a demo").path())
            .collect();
        folders.sort();
        let (mut sweep, mut proofs, mut compared) = (0, 0, 0);
        for folder in &folders {
            let entry = ["main.bend", "walkers_test.bend"]
                .into_iter()
                .find(|f| folder.join(f).is_file())
                .expect("a main.bend or a walkers_test.bend");
            let book = Book::open(folder, Path::new(entry)).expect("the book opens");
            compared += nothing_is_layer_one(&book, &folder.display().to_string());
            sweep += 1;
            if folder.join("PROOF.bend").is_file() {
                let book = Book::open(folder, Path::new("PROOF.bend")).expect("the book opens");
                compared += nothing_is_layer_one(&book, &folder.display().to_string());
                proofs += 1;
            }
        }
        assert_eq!(sweep, 16, "the 16 demo books");
        println!(
            "{sweep} books and {proofs} PROOF books, {compared} items, each as layer 1 has it"
        );
    }
}
