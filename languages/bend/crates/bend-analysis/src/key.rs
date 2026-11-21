//! What a context holds besides the def: the parameters a caller named, by position (`docs/08-bend/06-context-layer-design.md` §2 to §4). A key is closed (it names a def
//! or a lambda by identity and nothing of the caller), only the parameters the def calls or passes along are in it, and empty is the open key, which is layer 1's context.

use std::collections::BTreeSet;
use std::rc::Rc;

use analyzer_core::graph::StableDef;
use analyzer_core::outline::DefKind;
use serde::Serialize;

use crate::book::{DeclId, Resolved};
use crate::calls::{self, Bend, Precision};
use crate::env::Env;
use crate::sem::{ArgShape, What};

/// What a caller is known to have put in a function-shaped parameter. Closed: it names no variable of any def (BC-D1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Known {
    /// A def of the book with code, in the root.
    Def(DeclId),
    /// A lambda (`x => ..`, `\{..}`) written in `writer`'s body: the `nth` one the walk enters there, in source order.
    Lambda { writer: DeclId, nth: u32 },
}

/// What a context holds besides the def: the parameters a caller named, sorted by position, relevant ones only, never an unknown slot. Empty is the open key.
///
/// 🦀 A sorted slice and not a `BTreeMap`: a key is built once, then compared, hashed (the solver's memo is a `HashMap` keyed by it) and cloned. `Box<[T]>` is one pointer
/// and a length, `#[derive(Hash, Eq)]` works on it, and two keys with the same slots in the same order are equal; sorting by position at construction is what makes the
/// order canonical.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    slots: Box<[(u32, Known)]>,
}

impl Key {
    /// Nothing is known: the key of every context of layer 1 and of every entry of a request.
    pub(crate) fn open() -> Key {
        Key {
            slots: Box::new([]),
        }
    }

    /// A key of these slots, in any order. A position is given once.
    pub(crate) fn from_slots(mut slots: Vec<(u32, Known)>) -> Key {
        slots.sort_by_key(|(at, _)| *at);
        debug_assert!(slots.windows(2).all(|w| w[0].0 != w[1].0));
        Key {
            slots: slots.into_boxed_slice(),
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.slots.is_empty()
    }

    pub(crate) fn known(&self, position: u32) -> Option<&Known> {
        self.slots
            .binary_search_by_key(&position, |(at, _)| *at)
            .ok()
            .map(|at| &self.slots[at].1)
    }

    pub(crate) fn slots(&self) -> &[(u32, Known)] {
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
        }
    }
}

/// A def with code that is in the root: what a slot can hold (BC-D1). A law alone, a type, a def outside the root are not.
fn known_def(lang: &Bend<'_>, id: DeclId) -> bool {
    let (Some(decl), Some(file)) = (lang.book.decl(id), lang.book.file(id.file())) else {
        return false;
    };
    file.in_root && decl.kind == DefKind::Function
}

/// The relevant parameters of a def (BC-D2), and which of them the def itself calls.
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
    let all: BTreeSet<u32> = called.union(&passed).copied().collect();
    let found = Rc::new(Relevance {
        all: all.into_iter().collect(),
        called: called.into_iter().collect(),
    });
    lang.relevant.borrow_mut().insert(def, Rc::clone(&found));
    found
}

fn param_of(arg: &ArgShape) -> Option<u32> {
    match arg {
        ArgShape::Param(at) => Some(*at),
        _ => None,
    }
}

/// What the argument `arg` of a call written in `caller` puts in a slot (06 §4's table).
fn slot(
    lang: &Bend<'_>,
    caller: DeclId,
    arg: Option<&ArgShape>,
    env: &Env<'_>,
) -> Result<Known, Absent> {
    match arg {
        None => Err(Absent::Missing),
        Some(ArgShape::Lambda(nth)) => Ok(Known::Lambda {
            writer: caller,
            nth: *nth,
        }),
        // the caller's own parameter passed along: whatever the caller's key says for it
        Some(ArgShape::Param(at)) => env.known(*at).copied().ok_or(Absent::Open),
        Some(ArgShape::Local) => Err(Absent::Local),
        Some(ArgShape::Other) => Err(Absent::Value),
        Some(ArgShape::Name(name)) => match lang.book.resolve(caller.file(), name) {
            Resolved::Project(id) if known_def(lang, id) => Ok(Known::Def(id)),
            Resolved::Project(id)
                if lang
                    .book
                    .decl(id)
                    .is_some_and(|decl| decl.kind == DefKind::Enum) =>
            {
                Err(Absent::Type)
            }
            _ => Err(Absent::Value),
        },
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
    if lang.precision == Precision::Default {
        return Key::open();
    }
    let relevance = relevant(lang, callee);
    let slots = relevance
        .all
        .iter()
        .filter_map(|&at| match slot(lang, caller, args.get(at as usize), env) {
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

/// One slot written with nothing request-local in it: a def is its stable id, never a `DeclId` (an index into this request's book).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum Canon {
    Def(StableDef),
    Lambda(StableDef, u32),
}

/// A key written out with nothing request-local in it, in position order. A later `Known` adds a variant, and a later field of this struct is
/// `skip_serializing_if` its empty form, so a key that does not use it keeps the digest it had.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CanonKey {
    slots: Vec<(u32, Canon)>,
}

pub(crate) fn canon(lang: &Bend<'_>, key: &Key) -> CanonKey {
    CanonKey {
        slots: key
            .slots()
            .iter()
            .map(|(at, known)| {
                let canon = match known {
                    Known::Def(def) => Canon::Def(lang.stable_def(*def)),
                    Known::Lambda { writer, nth } => Canon::Lambda(lang.stable_def(*writer), *nth),
                };
                (*at, canon)
            })
            .collect(),
    }
}

/// The key a canonical form stands for in this book: each stable id looked up as an entry is (by marker, or by path, qualname and `nth`). `None` when one of the
/// defs is gone: the key cannot be rebuilt, and the caller starts from the open key and says so.
pub(crate) fn rebuild(lang: &Bend<'_>, canon: &CanonKey) -> Option<Key> {
    let slots = canon
        .slots
        .iter()
        .map(|(at, slot)| {
            let known = match slot {
                Canon::Def(def) => Known::Def(lang.decl_of(def)?),
                Canon::Lambda(writer, nth) => Known::Lambda {
                    writer: lang.decl_of(writer)?,
                    nth: *nth,
                },
            };
            Some((*at, known))
        })
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
        let a = Key::from_slots(vec![(2, Known::Def(each)), (0, Known::Def(work))]);
        let b = Key::from_slots(vec![(0, Known::Def(work)), (2, Known::Def(each))]);
        assert_eq!(a, b);
        assert_eq!(a.known(0), Some(&Known::Def(work)));
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
                Known::Lambda {
                    writer: each,
                    nth: 3,
                },
            ),
            (0, Known::Def(work)),
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
        let other = Key::from_slots(vec![(0, Known::Def(each))]);
        assert_ne!(digest(&lang, &key), digest(&lang, &other));
    }

    #[test]
    fn a_parameter_is_relevant_when_called_or_handed_to_a_project_def() {
        let root = scratch("key-relevant");
        write(root.join("main.bend"), PROGRAM);
        let book = Book::open(&root, std::path::Path::new("main.bend")).expect("opens");
        let lang = Bend::new(&book, MarkerStyle::above("ID"), Precision::Precise);
        let of = |name: &str| relevant(&lang, id(&book, name)).all.to_vec();
        let called = |name: &str| relevant(&lang, id(&book, name)).called.to_vec();
        assert_eq!(of("work"), Vec::<u32>::new(), "never called, never passed");
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
