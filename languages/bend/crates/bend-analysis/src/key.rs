//! What a context holds besides the def: the parameters a caller named, by position (`docs/08-bend/06-context-layer-design.md` §2 to §4). A key is closed (it names a def
//! or a lambda by identity and nothing of the caller), only the parameters the def calls or passes along are in it, and empty is the open key, which is layer 1's context.

use std::collections::BTreeSet;
use std::rc::Rc;

use analyzer_core::graph::StableDef;
use analyzer_core::outline::DefKind;
use serde::Serialize;

use crate::book::{DeclId, FileIndex, Resolved};
use crate::calls::{self, Bend, Precision};
use crate::env::Env;
use crate::sem::{ArgShape, Expr, What};
use crate::value::{One, Value};

/// What a context holds besides the def: the parameters a caller named, sorted by position, relevant ones only, never an unknown slot and never data. Empty is the open key.
///
/// 🦀 A sorted slice and not a `BTreeMap`: a key is built once, then compared, hashed (the solver's memo is a `HashMap` keyed by it) and cloned. `Box<[T]>` is one pointer
/// and a length, `#[derive(Hash, Eq)]` works on it, and two keys with the same slots in the same order are equal; sorting by position at construction is what makes the
/// order canonical.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    slots: Box<[(u32, Value)]>,
}

impl Key {
    /// Nothing is known: the key of every context of layer 1 and of every entry of a request.
    pub(crate) fn open() -> Key {
        Key {
            slots: Box::new([]),
        }
    }

    /// A key of these slots, in any order. A position is given once.
    pub(crate) fn from_slots(mut slots: Vec<(u32, Value)>) -> Key {
        slots.sort_by_key(|(at, _)| *at);
        debug_assert!(slots.windows(2).all(|w| w[0].0 != w[1].0));
        Key {
            slots: slots.into_boxed_slice(),
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.slots.is_empty()
    }

    pub(crate) fn known(&self, position: u32) -> Option<&Value> {
        self.slots
            .binary_search_by_key(&position, |(at, _)| *at)
            .ok()
            .map(|at| &self.slots[at].1)
    }

    pub(crate) fn slots(&self) -> &[(u32, Value)] {
        &self.slots
    }
}

/// Why a relevant position has no slot in a key: the count of each is kept per context (`key_slot_*`, from step 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Absent {
    /// The argument is the caller's own parameter and the caller's key does not know it.
    Open,
    /// The argument is a type: already counted `type_passed`.
    Type,
    /// A `let`, a `case` variable, a lambda's binder, a do's name.
    Local,
    /// A call's result, a constructor, a literal, an operator, a Base def, a hub's, a def outside the root.
    Value,
    /// The call has no argument at that position (partial application).
    Missing,
    /// The position is a `do` statement's continuation, which is never in a key (07 §8): nothing is counted.
    Stays,
}

impl Absent {
    /// The counter of `Stats.language` this absence is counted in; a type is not counted (`type_passed` has it).
    fn counter(self) -> Option<&'static str> {
        match self {
            Absent::Open => Some(calls::KEY_SLOT_OPEN),
            Absent::Type => None,
            Absent::Local => Some(calls::KEY_SLOT_LOCAL),
            Absent::Value => Some(calls::KEY_SLOT_VALUE),
            Absent::Missing => Some(calls::KEY_SLOT_MISSING),
            Absent::Stays => None,
        }
    }
}

/// A def with code that is in the root: what a slot can hold (BC-D1). A law alone, a type, a def outside the root are not.
pub(crate) fn known_def(lang: &Bend<'_>, id: DeclId) -> bool {
    let (Some(decl), Some(file)) = (lang.book.decl(id), lang.book.file(id.file())) else {
        return false;
    };
    file.in_root && decl.kind == DefKind::Function
}

/// The relevant parameters of a def (BC-D2, and the value of a `let`, V3), and which of them the def itself calls.
pub(crate) struct Relevance {
    /// Every relevant position: called (a), or passed along to a project def (b) or through a parameter (c). A key has a slot for each one a caller names.
    pub(crate) all: Box<[u32]>,
    /// The positions the def calls (a): a missing slot there leaves a site of the def unresolved, so only these are counted (`key_slot_*`, 06 §4). A subset of `all`.
    called: Box<[u32]>,
}

impl Relevance {
    fn is_called(&self, position: u32) -> bool {
        self.called.binary_search(&position).is_ok()
    }
}

/// The parameters of `def` that are **relevant** (BC-D2): the def calls one (`f(x)`), or passes one as a direct argument to a call of a project def or to a call
/// through one of its own parameters (`h(f, x)`: when a caller names `h`, `f` flows down into it). Positions of the written parameter list, sorted. A fact of the
/// def's own code, so computed once per def and kept.
pub(crate) fn relevant(lang: &Bend<'_>, def: DeclId) -> Rc<Relevance> {
    if let Some(known) = lang.relevant.borrow().get(&def) {
        return Rc::clone(known);
    }
    let mut called: BTreeSet<u32> = BTreeSet::new();
    let mut passed: BTreeSet<u32> = BTreeSet::new();
    for item in lang.items(def) {
        match &item.what {
            // (a), and (c): a call through the def's own parameter hands its argument parameters on to whatever a caller names
            What::Opaque {
                param: Some(at),
                args,
            } => {
                called.insert(*at);
                passed.extend(args.iter().filter_map(param_of));
            }
            // (b)
            What::Call { name, args }
                if matches!(lang.book.resolve(def.file(), name), Resolved::Project(_)) =>
            {
                passed.extend(args.iter().filter_map(param_of));
            }
            _ => {}
        }
    }
    // (d) a parameter that is the value of a `let` is read through the variable: `g = f`, then `g(x)` calls what a caller named for `f`; (e) a parameter that reaches the
    // reply is the value of every call of the def
    if let Some(walk) = lang.walk_of(def) {
        let params = walk.flow.params;
        let file = def.file();
        value_params(lang, file, &walk.flow.body, params, &mut passed);
        walk.flow.body.walk(&mut |expr| match expr {
            Expr::Let { values, .. } => values
                .iter()
                .for_each(|value| value_params(lang, file, value, params, &mut passed)),
            // (f) a parameter that is a scrutinee is read through the patterns: a constructor's fields are the names it binds
            Expr::Match { on, .. } => on
                .iter()
                .for_each(|value| value_params(lang, file, value, params, &mut passed)),
            _ => {}
        });
    }
    let all: BTreeSet<u32> = called.union(&passed).copied().collect();
    let found = Rc::new(Relevance {
        all: all.into_iter().collect(),
        called: called.into_iter().collect(),
    });
    lang.relevant.borrow_mut().insert(def, Rc::clone(&found));
    found
}

/// The parameters (binders below `params`) that `expr` can have as its value: itself when it is one, and what the cases of a `match` and the end of a `let` or a sequence
/// can have.
fn value_params(
    lang: &Bend<'_>,
    file: FileIndex,
    expr: &Expr,
    params: u32,
    out: &mut BTreeSet<u32>,
) {
    match expr {
        Expr::Var(n) if *n < params => {
            out.insert(*n);
        }
        Expr::Match { cases, .. } => cases
            .iter()
            .for_each(|(_, body)| value_params(lang, file, body, params, out)),
        Expr::Let { rest, .. } => value_params(lang, file, rest, params, out),
        Expr::Seq(parts) => {
            if let Some(last) = parts.last() {
                value_params(lang, file, last, params, out);
            }
        }
        // a constructor of a project type holds its fields' values (07 §9); one of Base's holds nothing the layer can read
        Expr::Ctr { name, fields, .. } => {
            if matches!(lang.book.ctor(file, name), Resolved::Project(_)) {
                fields
                    .iter()
                    .for_each(|field| value_params(lang, file, field, params, out));
            }
        }
        _ => {}
    }
}

fn param_of(arg: &ArgShape) -> Option<u32> {
    match arg {
        ArgShape::Param(at) => Some(*at),
        _ => None,
    }
}

/// What the argument `arg` of a call written in `caller` puts in a slot (06 §4's table), as the item walk reads it off the shapes.
fn slot(
    lang: &Bend<'_>,
    caller: DeclId,
    arg: Option<&ArgShape>,
    env: &Env<'_>,
) -> Result<Value, Absent> {
    match arg {
        None => Err(Absent::Missing),
        Some(ArgShape::Lambda(nth)) => Ok(Value::one(One::Lambda {
            writer: caller,
            nth: *nth,
        })),
        // the caller's own parameter passed along: whatever the caller's key says for it
        Some(ArgShape::Param(at)) => env.known(*at).cloned().ok_or(Absent::Open),
        Some(ArgShape::Local) => Err(Absent::Local),
        Some(ArgShape::Other) => Err(Absent::Value),
        Some(ArgShape::Name(name)) => match lang.book.resolve(caller.file(), name) {
            Resolved::Project(id) if known_def(lang, id) => Ok(Value::one(One::Def(id))),
            _ => Err(absent_name(lang, caller, name)),
        },
    }
}

/// Why a bare name in an argument is not a slot when it is not a def with code: a type is a type argument, anything else is a value.
pub(crate) fn absent_name(lang: &Bend<'_>, caller: DeclId, name: &str) -> Absent {
    match lang.book.resolve(caller.file(), name) {
        Resolved::Project(id)
            if lang
                .book
                .decl(id)
                .is_some_and(|decl| decl.kind == DefKind::Enum) =>
        {
            Absent::Type
        }
        _ => Absent::Value,
    }
}

/// The key of a call of `callee` written in `caller`, from the arguments as `sem` read them (06 §4): one slot for each relevant position of the callee whose argument is
/// known; each one that is not is counted for the context being walked (`key_slot_*`) when the callee calls that position (06 §4: a count of a position it only passes
/// along says nothing). Under `Precision::Default` it is the open key and nothing is read.
pub(crate) fn key_of(
    lang: &Bend<'_>,
    caller: DeclId,
    callee: DeclId,
    args: &[ArgShape],
    env: &Env<'_>,
) -> Key {
    key_with(lang, callee, |at| {
        slot(lang, caller, args.get(at as usize), env)
    })
}

/// The same key from whatever says what the argument at a position puts in its slot: the item walk reads it off the shapes (`key_of`), the evaluator off the values
/// of the arguments (`eval.rs`). The counting is one thing, here.
pub(crate) fn key_with(
    lang: &Bend<'_>,
    callee: DeclId,
    slot: impl Fn(u32) -> Result<Value, Absent>,
) -> Key {
    if lang.precision == Precision::Default {
        return Key::open();
    }
    let relevance = relevant(lang, callee);
    let slots = relevance
        .all
        .iter()
        .filter_map(|&at| match slot(at) {
            Ok(known) => Some((at, known)),
            Err(absent) => {
                if relevance.is_called(at)
                    && let Some(counter) = absent.counter()
                {
                    lang.note_slot(counter);
                }
                None
            }
        })
        .collect();
    Key::from_slots(slots)
}

/// One slot written with nothing request-local in it: a def is its stable id, never a `DeclId` (an index into this request's book). A value that is exactly one
/// closed def or lambda is written as it always was (`Def`, `Lambda`), so no digest made before moves; any other is a `Set` of what it can be and whether something
/// else can be there too.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum Canon {
    Def(StableDef),
    Lambda(StableDef, u32),
    /// A def and what it was given, by position from the first (a value that is not exactly one def or lambda is a `Set`).
    Partial(StableDef, Vec<Canon>),
    /// A constructor of a type, by the stable id of the type and the name of the constructor, and what it was given.
    Ctor(StableDef, String, Vec<Canon>),
    Set {
        known: Vec<Canon>,
        open: bool,
    },
}

/// A key written out with nothing request-local in it, in position order. A later field of this struct is `skip_serializing_if` its empty form, so a key that does not use it
/// keeps the digest it had.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CanonKey {
    slots: Vec<(u32, Canon)>,
}

fn canon_one(lang: &Bend<'_>, one: &One) -> Canon {
    match one {
        One::Def(def) => Canon::Def(lang.stable_def(*def)),
        One::Lambda { writer, nth } => Canon::Lambda(lang.stable_def(*writer), *nth),
        One::Partial { def, args } => Canon::Partial(
            lang.stable_def(*def),
            args.iter().map(|arg| canon_value(lang, arg)).collect(),
        ),
        One::Ctor { ty, name, fields } => Canon::Ctor(
            lang.stable_def(*ty),
            name.to_string(),
            fields
                .iter()
                .map(|field| canon_value(lang, field))
                .collect(),
        ),
    }
}

fn canon_value(lang: &Bend<'_>, value: &Value) -> Canon {
    match value.single() {
        Some(one) => canon_one(lang, one),
        None => Canon::Set {
            known: value
                .members()
                .iter()
                .map(|one| canon_one(lang, one))
                .collect(),
            open: value.is_open(),
        },
    }
}

pub(crate) fn canon(lang: &Bend<'_>, key: &Key) -> CanonKey {
    CanonKey {
        slots: key
            .slots()
            .iter()
            .map(|(at, value)| (*at, canon_value(lang, value)))
            .collect(),
    }
}

fn rebuild_one(lang: &Bend<'_>, canon: &Canon) -> Option<One> {
    Some(match canon {
        Canon::Def(def) => One::Def(lang.decl_of(def)?),
        Canon::Lambda(writer, nth) => One::Lambda {
            writer: lang.decl_of(writer)?,
            nth: *nth,
        },
        Canon::Partial(def, args) => One::Partial {
            def: lang.decl_of(def)?,
            args: args
                .iter()
                .map(|arg| rebuild_value(lang, arg))
                .collect::<Option<Vec<_>>>()?
                .into(),
        },
        Canon::Ctor(ty, name, fields) => One::Ctor {
            ty: lang.decl_of(ty)?,
            name: name.as_str().into(),
            fields: fields
                .iter()
                .map(|field| rebuild_value(lang, field))
                .collect::<Option<Vec<_>>>()?
                .into(),
        },
        Canon::Set { .. } => return None,
    })
}

fn rebuild_value(lang: &Bend<'_>, canon: &Canon) -> Option<Value> {
    match canon {
        Canon::Set { known, open } => {
            let mut value = if *open {
                Value::unknown()
            } else {
                Value::data()
            };
            for one in known {
                value = value.join(&Value::one(rebuild_one(lang, one)?))?;
            }
            Some(value)
        }
        single => Some(Value::one(rebuild_one(lang, single)?)),
    }
}

/// The key a canonical form stands for in this book: each stable id looked up as an entry is (by marker, or by path, qualname and `nth`). `None` when one of the
/// defs is gone: the key cannot be rebuilt, and the caller starts from the open key and says so.
pub(crate) fn rebuild(lang: &Bend<'_>, canon: &CanonKey) -> Option<Key> {
    let slots = canon
        .slots
        .iter()
        .map(|(at, slot)| Some((*at, rebuild_value(lang, slot)?)))
        .collect::<Option<Vec<_>>>()?;
    Some(Key::from_slots(slots))
}

/// The 16 bytes of `ContextId::digest`: zeros for the open key (layer 1's id), else `xxh3_128` of the canonical key as JSON, little-endian. Bytes we wrote, never
/// `#[derive(Hash)]`: std makes no promise to keep it stable.
pub(crate) fn digest(lang: &Bend<'_>, key: &Key) -> [u8; 16] {
    if key.is_open() {
        return [0; 16];
    }
    let bytes = serde_json::to_vec(&canon(lang, key)).expect("a canonical key always serializes");
    xxhash_rust::xxh3::xxh3_128(&bytes).to_le_bytes()
}

#[cfg(test)]
mod tests {
    use analyzer_core::edit::MarkerStyle;

    use super::*;
    use crate::book::Book;
    use crate::scratch::{scratch, write};

    const PROGRAM: &str = concat!(
        "def work(x: U32) -> U32:\n  x\n\n",
        "def each(f: U32 -> U32, x: U32) -> U32:\n  f(x)\n\n",
        "def relay(f: U32 -> U32, x: U32) -> U32:\n  each(f, x)\n\n",
        "def boxed(f: U32 -> U32, x: U32) -> U32:\n  Box{f}\n\n",
        "def shown(f: U32 -> U32, x: U32) -> U32:\n  U32.show(f)\n\n",
        "def two(~a: U32 -> U32, b: U32 -> U32, c: U32) -> U32:\n  a(b(c))\n\n",
        "def lam(f: U32 -> U32) -> U32:\n  each(y => f(y), 1)\n\n",
        "def main() -> U32:\n  each(work, 1)\n",
    );

    fn id(book: &Book, qualname: &str) -> DeclId {
        book.decls()
            .iter()
            .find(|d| book.def(d.id).is_some_and(|def| def.qualname == qualname))
            .expect("a def of that name")
            .id
    }

    #[test]
    fn a_key_is_sorted_by_position_and_empty_is_open() {
        let book = {
            let root = scratch("key-order");
            write(root.join("main.bend"), PROGRAM);
            Book::open(&root, std::path::Path::new("main.bend")).expect("opens")
        };
        let (work, each) = (id(&book, "work"), id(&book, "each"));
        let (work, each) = (Value::one(One::Def(work)), Value::one(One::Def(each)));
        let a = Key::from_slots(vec![(2, each.clone()), (0, work.clone())]);
        let b = Key::from_slots(vec![(0, work.clone()), (2, each)]);
        assert_eq!(a, b);
        assert_eq!(a.known(0), Some(&work));
        assert_eq!(a.known(1), None);
        assert!(Key::open().is_open() && !a.is_open());
        assert_eq!(Key::from_slots(Vec::new()), Key::open());
    }

    /// The bytes the digest hashes are written here by hand: a change to them changes every id a driver has stored.
    #[test]
    fn the_canonical_form_and_the_digest_of_a_key() {
        let root = scratch("key-canon");
        write(root.join("main.bend"), PROGRAM);
        let book = Book::open(&root, std::path::Path::new("main.bend")).expect("opens");
        let lang = Bend::new(&book, MarkerStyle::above("ID"), Precision::Precise);
        let (work, each) = (id(&book, "work"), id(&book, "each"));
        let key = Key::from_slots(vec![
            (
                1,
                Value::one(One::Lambda {
                    writer: each,
                    nth: 3,
                }),
            ),
            (0, Value::one(One::Def(work))),
        ]);
        let json = serde_json::to_string(&canon(&lang, &key)).unwrap();
        assert_eq!(
            json,
            concat!(
                r#"{"slots":[[0,{"Def":{"Path":{"module":"main.bend","qname":"work","nth":0}}}],"#,
                r#"[1,{"Lambda":[{"Path":{"module":"main.bend","qname":"each","nth":0}},3]}]]}"#
            )
        );
        assert_eq!(
            digest(&lang, &key),
            xxhash_rust::xxh3::xxh3_128(json.as_bytes()).to_le_bytes()
        );
        assert_eq!(
            digest(&lang, &Key::open()),
            [0; 16],
            "the open key is zeros, not the hash of nothing"
        );
        let other = Key::from_slots(vec![(0, Value::one(One::Def(each)))]);
        assert_ne!(digest(&lang, &key), digest(&lang, &other));
    }

    /// A slot that holds a set, or a def and something else, is a `Set`, written by hand here: the closed def or lambda of one member is not (the test above).
    #[test]
    fn the_canonical_form_of_a_set_and_of_a_def_that_something_else_may_be_beside() {
        let root = scratch("key-canon-set");
        write(root.join("main.bend"), PROGRAM);
        let book = Book::open(&root, std::path::Path::new("main.bend")).expect("opens");
        let lang = Bend::new(&book, MarkerStyle::above("ID"), Precision::Precise);
        let (work, each) = (id(&book, "work"), id(&book, "each"));
        let both = Value::one(One::Def(work))
            .join(&Value::one(One::Def(each)))
            .unwrap();
        let key = Key::from_slots(vec![
            (0, both.clone()),
            (
                2,
                Value::one(One::Def(work)).join(&Value::unknown()).unwrap(),
            ),
        ]);
        let json = serde_json::to_string(&canon(&lang, &key)).unwrap();
        assert_eq!(
            json,
            concat!(
                r#"{"slots":[[0,{"Set":{"known":[{"Def":{"Path":{"module":"main.bend","qname":"work","nth":0}}},"#,
                r#"{"Def":{"Path":{"module":"main.bend","qname":"each","nth":0}}}],"open":false}}],"#,
                r#"[2,{"Set":{"known":[{"Def":{"Path":{"module":"main.bend","qname":"work","nth":0}}}],"open":true}}]]}"#
            )
        );
        // and it comes back as the key it was
        assert_eq!(rebuild(&lang, &canon(&lang, &key)), Some(key));
    }

    /// A def that was given some of its arguments is a `Partial` of the def and what it was given, each as a value: a number is data, the empty closed set.
    #[test]
    fn the_canonical_form_of_a_partial_application() {
        let root = scratch("key-canon-partial");
        write(root.join("main.bend"), PROGRAM);
        let book = Book::open(&root, std::path::Path::new("main.bend")).expect("opens");
        let lang = Bend::new(&book, MarkerStyle::above("ID"), Precision::Precise);
        let (work, each) = (id(&book, "work"), id(&book, "each"));
        let partial = Value::one(One::Partial {
            def: each,
            args: vec![Value::one(One::Def(work)), Value::data()].into(),
        });
        let key = Key::from_slots(vec![(1, partial)]);
        let json = serde_json::to_string(&canon(&lang, &key)).unwrap();
        assert_eq!(
            json,
            concat!(
                r#"{"slots":[[1,{"Partial":[{"Path":{"module":"main.bend","qname":"each","nth":0}},"#,
                r#"[{"Def":{"Path":{"module":"main.bend","qname":"work","nth":0}}},{"Set":{"known":[],"open":false}}]]}]]}"#
            )
        );
        assert_eq!(rebuild(&lang, &canon(&lang, &key)), Some(key));
    }

    /// A constructor of a type is the stable id of the type, the name of the constructor and what it was given.
    #[test]
    fn the_canonical_form_of_a_constructor() {
        let root = scratch("key-canon-ctor");
        write(root.join("main.bend"), PROGRAM);
        let book = Book::open(&root, std::path::Path::new("main.bend")).expect("opens");
        let lang = Bend::new(&book, MarkerStyle::above("ID"), Precision::Precise);
        let (work, each) = (id(&book, "work"), id(&book, "each"));
        let object = Value::one(One::Ctor {
            ty: each,
            name: "Game".into(),
            fields: vec![Value::one(One::Def(work)), Value::unknown()].into(),
        });
        let key = Key::from_slots(vec![(0, object)]);
        let json = serde_json::to_string(&canon(&lang, &key)).unwrap();
        assert_eq!(
            json,
            concat!(
                r#"{"slots":[[0,{"Ctor":[{"Path":{"module":"main.bend","qname":"each","nth":0}},"Game","#,
                r#"[{"Def":{"Path":{"module":"main.bend","qname":"work","nth":0}}},{"Set":{"known":[],"open":true}}]]}]]}"#
            )
        );
        assert_eq!(rebuild(&lang, &canon(&lang, &key)), Some(key));
    }

    #[test]
    fn a_parameter_is_relevant_when_called_or_handed_to_a_project_def() {
        let root = scratch("key-relevant");
        write(root.join("main.bend"), PROGRAM);
        let book = Book::open(&root, std::path::Path::new("main.bend")).expect("opens");
        let lang = Bend::new(&book, MarkerStyle::above("ID"), Precision::Precise);
        let of = |name: &str| relevant(&lang, id(&book, name)).all.to_vec();
        let called = |name: &str| relevant(&lang, id(&book, name)).called.to_vec();
        assert_eq!(
            of("work"),
            [0],
            "never called, never passed, but it is the reply (V6): a caller's value for it is what the call returns"
        );
        assert_eq!(
            of("each"),
            [0, 1],
            "`f(x)`: `f` is called and `x` is handed to it (c)"
        );
        assert_eq!(
            of("relay"),
            [0, 1],
            "`each(f, x)`: both passed along (the over-approximation of 06 §3.2)"
        );
        assert_eq!(of("boxed"), Vec::<u32>::new(), "a constructor's field");
        assert_eq!(of("shown"), Vec::<u32>::new(), "a Base def's argument");
        assert_eq!(
            of("two"),
            [0, 1, 2],
            "`a(b(c))`: `a` and `b` are called, `c` is handed to `b` (c)"
        );
        // the positions the def calls itself, which are the ones a missing slot is counted at
        assert_eq!(called("each"), [0]);
        assert_eq!(called("relay"), Vec::<u32>::new());
        assert_eq!(called("two"), [0, 1]);
        assert_eq!(
            of("lam"),
            [0],
            "a lambda's call of the writer's parameter counts for the writer"
        );
    }
}
