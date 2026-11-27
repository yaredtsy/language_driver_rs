//! What a context holds besides the function: its key, interned (`docs/03-typescript/12-context-layer/03-design.md` §6). A key is a number into
//! the request's `Keys`; what it holds is a `KeyData` in its normal form: the values of the parameters (`crate::value::Value`). The slots are filled by
//! `bind` at each project target when the layer knows `Caps::PARAMS` (`walk::key_of`); until then the only key is the open one, `KeyId::OPEN`, with digest
//! zeros. Every other key's digest is core's `digest` of its canonical form, made once, when the key is first met (`docs/02-tree-rules/08-the-digest-and-scenario-levels.md` §1).

use indexmap::IndexSet;
use serde::Serialize;

use analyzer_core::graph::{StableDef, digest};

use crate::flow::{Arg, DefId, Expr, Param, VarId};
use crate::nested::{CaptureList, ObjectData};
use crate::value::{Captures, ObjectId, One, Value};

/// The version of a context's digest under `Precision::Precise`: the digest is core's `digest` of the canonical form of the key, and this says which
/// form. It goes up when the digest of a key that already exists must change; a new variant, or a field that is skipped when empty, does not change the
/// digest of any key that exists, so it does not bump it. The tag notes say so when it goes up.
pub const CONTEXT_ID_VERSION: u32 = 1;

/// A key: its number in the request's `Keys`. Two keys of one request are equal exactly when their numbers are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct KeyId(u32);

impl KeyId {
    /// Everything unknown: the key of a request's entry always, and of every context until the layer knows a value. Its digest is zeros, so a
    /// context under it has the id it has without the layer.
    pub(crate) const OPEN: KeyId = KeyId(0);
}

/// The key the solver is keyed by (`Lang::Key`).
pub(crate) type Key = KeyId;

/// What a key holds, in its normal form.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct KeyData {
    /// One per parameter, in order (`Flow::params`): what the caller passes. TC2 fills it; the receiver comes with TC5.
    pub(crate) params: Box<[Value]>,
    /// What the context has for the variables of the functions around the callee (`07-tc3` §4), sorted by `VarId`, no unknown entry: a variable not listed is
    /// unknown. TC3 step 3 fills it.
    pub(crate) captured: Box<[(VarId, Value)]>,
}

impl KeyData {
    /// The one normal form (`03-design.md` §6), made by `Keys::intern` and nowhere else: every `One::Anon(..)` of every slot and of every captured value is
    /// written as `One::AnonArrow`, then the trailing parameters that are `Value::unknown()` are dropped, so the open key of a function with three parameters
    /// equals that of one with none; the captured entries are sorted by variable and the unknown ones dropped.
    fn normal(mut self) -> KeyData {
        // an anonymous function is one member, whichever it is: callers that pass different arrows share a context
        for slot in self.params.iter_mut() {
            if slot.known().iter().any(|one| matches!(one, One::Anon(..))) {
                *slot = slot.map(|one| match one {
                    One::Anon(..) => One::AnonArrow,
                    other => other,
                });
            }
        }
        let unknown = Value::unknown();
        if self.captured.iter().any(|(_, value)| {
            *value == unknown || value.known().iter().any(|one| matches!(one, One::Anon(..)))
        }) || !self.captured.windows(2).all(|pair| pair[0].0 < pair[1].0)
        {
            let mut kept: Vec<(VarId, Value)> = self
                .captured
                .iter()
                .filter(|(_, value)| *value != unknown)
                .map(|(var, value)| {
                    let value = value.map(|one| match one {
                        One::Anon(..) => One::AnonArrow,
                        other => other,
                    });
                    (*var, value)
                })
                .collect();
            kept.sort_by_key(|(var, _)| *var);
            self.captured = kept.into();
        }
        let keep = self
            .params
            .iter()
            .rposition(|value| *value != unknown)
            .map_or(0, |last| last + 1);
        if keep < self.params.len() {
            self.params = self.params[..keep].into();
        }
        self
    }
}

/// Whether a parameter's slot can hold a value in a key (`06-tc2` §4): it is capable and not a rest parameter. The others are unknown in every key.
fn admits(param: &Param) -> bool {
    param.capable && !param.rest
}

/// The key a call passes: argument `i` bound to parameter `i` of the callee's own list (`Flow::params`), by position, as JavaScript binds it
/// (`06-tc2` §5). Slot `i` is unknown when the parameter is not admitted or not relevant (`relevant[i]` is false, or `relevant` has no `i`: its argument is
/// not evaluated) or when a spread is at an argument at or before `i`; else the argument's value (`value_of`), or `{Undefined}` when the call has no
/// argument `i`. Arguments past the last parameter are not read.
pub(crate) fn bind(
    params: &[Param],
    args: &[Arg],
    relevant: &[bool],
    mut value_of: impl FnMut(&Expr) -> Value,
) -> KeyData {
    let spread = args.iter().position(|arg| arg.spread);
    let params = params
        .iter()
        .enumerate()
        .map(|(i, param)| {
            if !admits(param)
                || !relevant.get(i).copied().unwrap_or(false)
                || spread.is_some_and(|at| at <= i)
            {
                Value::unknown()
            } else if let Some(arg) = args.get(i) {
                value_of(&arg.value)
            } else {
                Value::of([One::Undefined], false)
            }
        })
        .collect();
    KeyData {
        params,
        captured: Box::new([]),
    }
}

/// A key written with stable ids only, for its digest (`03-design.md` §6). The canonical form only grows: a later phase adds a variant, or a field of
/// `CanonKey` skipped when empty, so a key that does not use it keeps its digest. Each variant's bytes are pinned by a test.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
enum Canon {
    Unknown,
    Data,
    Wide,
    Fn(StableDef),
    /// An anonymous arrow, which one not said (`One::AnonArrow`). 🦀 `serde(rename)` changes only the name written out, not the Rust name: the bytes (and every digest holding an arrow) stay as `AnonAny` wrote them
    #[serde(rename = "AnonAny")]
    AnonArrow,
    Undefined,
    /// Two or more members, or one and open: sorted as `Canon` (by `StableDef`), never by `DefId`.
    Set {
        known: Vec<Canon>,
        open: bool,
    },
    /// A nested def and what it captured (`07-tc3` §4): each variable with its value, sorted by the variable's canonical form.
    Closure {
        func: StableDef,
        captured: Vec<(CanonVar, Canon)>,
    },
    /// An object written by its members (in name order, as the table keeps them) and never by its number: its place in a request's table follows the order
    /// objects were made.
    Object {
        members: Vec<(String, Canon)>,
        complete: bool,
    },
    /// An anonymous function expression, which one not said (`One::AnonFunction`). Last, as the canonical form only grows.
    AnonFunction,
}

/// A captured variable written with stable ids only: the def that declares it, its name, and which row of that name it is among the rows of that def (by
/// place), so two variables of one name in one def are told apart, and an edit above them moves neither.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct CanonVar {
    pub(crate) def: StableDef,
    pub(crate) name: String,
    pub(crate) nth: u16,
}

#[derive(Debug, Serialize)]
struct CanonKey {
    params: Vec<Canon>,
    /// What the context has for the variables around the callee; skipped when empty, so every key without one keeps the digest it had.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    captured: Vec<(CanonVar, Canon)>,
}

/// What a key's canonical form asks of the world: the stable id of a def, the canonical form of a variable, the list a closure holds, and the object a number
/// stands for.
struct Names<'a, S, V, L, O> {
    stable: &'a S,
    stable_var: &'a V,
    list: &'a L,
    objects: &'a O,
}

impl CanonKey {
    fn of<S, V, L, O>(data: &KeyData, names: &Names<'_, S, V, L, O>) -> CanonKey
    where
        S: Fn(DefId) -> StableDef,
        V: Fn(VarId) -> CanonVar,
        L: Fn(Captures) -> CaptureList,
        O: Fn(ObjectId) -> ObjectData,
    {
        CanonKey {
            params: data.params.iter().map(|slot| canon(slot, names)).collect(),
            captured: captured_canon(&data.captured, names),
        }
    }
}

/// `list` in its canonical form: sorted by the variable's canonical form, not by `VarId`, whose order follows the order files were first asked for.
fn captured_canon<S, V, L, O>(
    list: &[(VarId, Value)],
    names: &Names<'_, S, V, L, O>,
) -> Vec<(CanonVar, Canon)>
where
    S: Fn(DefId) -> StableDef,
    V: Fn(VarId) -> CanonVar,
    L: Fn(Captures) -> CaptureList,
    O: Fn(ObjectId) -> ObjectData,
{
    let mut written: Vec<(CanonVar, Canon)> = list
        .iter()
        .map(|(var, value)| ((names.stable_var)(*var), canon(value, names)))
        .collect();
    written.sort();
    written
}

/// A slot in its canonical form: `stable` names each def it holds. 🦀 `Canon` derives `Ord`, so `sort()` orders the members by variant and then by
/// `StableDef`: the order does not depend on `DefId`, whose file numbers follow the order files were first asked for.
fn canon<S, V, L, O>(value: &Value, names: &Names<'_, S, V, L, O>) -> Canon
where
    S: Fn(DefId) -> StableDef,
    V: Fn(VarId) -> CanonVar,
    L: Fn(Captures) -> CaptureList,
    O: Fn(ObjectId) -> ObjectData,
{
    if value.is_wide() {
        return Canon::Wide;
    }
    let mut known: Vec<Canon> = value
        .known()
        .iter()
        .map(|one| match one {
            One::Fn(def) => Canon::Fn((names.stable)(*def)),
            One::Closure(def, captures) => Canon::Closure {
                func: (names.stable)(*def),
                captured: captured_canon(&(names.list)(*captures), names),
            },
            One::AnonArrow => Canon::AnonArrow,
            One::AnonFunction => Canon::AnonFunction,
            One::Undefined => Canon::Undefined,
            One::Object(id) => {
                let object = (names.objects)(*id);
                Canon::Object {
                    members: object
                        .members
                        .iter()
                        .map(|(name, value)| (name.to_string(), canon(value, names)))
                        .collect(),
                    complete: object.complete,
                }
            }
            One::Anon(..) => {
                debug_assert!(
                    false,
                    "a key's anonymous function is AnonArrow (KeyData::normal)"
                );
                Canon::AnonArrow
            }
        })
        .collect();
    match (known.len(), value.is_open()) {
        (0, true) => Canon::Unknown,
        (0, false) => Canon::Data,
        (1, false) => known.remove(0),
        _ => {
            known.sort();
            Canon::Set {
                known,
                open: value.is_open(),
            }
        }
    }
}

/// Every key of one request, each once: its number is its place in `table`, and `digests` holds its digest at the same place, made once, when
/// the key is first met. Numbers follow the order keys are first met, which is the solver's order, so they are the same on every run (rule 9).
pub(crate) struct Keys {
    table: IndexSet<KeyData>,
    digests: Vec<[u8; 16]>,
    /// The digests made so far, to check in a debug build that no two keys share one (a set, so the check costs one lookup, not a pass over `digests`).
    #[cfg(debug_assertions)]
    seen: std::collections::HashSet<[u8; 16]>,
}

impl Keys {
    /// The table of a new request: the open key alone, number 0, with digest zeros. Its digest is never hashed: the zeros are the language's rule
    /// (`docs/02-tree-rules/08-the-digest-and-scenario-levels.md` §1).
    pub(crate) fn new() -> Keys {
        let mut table = IndexSet::new();
        let (open, new) = table.insert_full(KeyData::default());
        debug_assert!(
            new && open == KeyId::OPEN.0 as usize,
            "the open key is number 0"
        );
        Keys {
            table,
            digests: vec![[0; 16]],
            #[cfg(debug_assertions)]
            seen: std::collections::HashSet::new(),
        }
    }

    /// The number of `data`, in its normal form: the one it already has, or the next one, whose digest is made here, once: core's `digest` of its
    /// canonical form, `stable` naming each def a slot holds, `stable_var` each variable a closure or the key captured, `list` the list a closure holds, and
    /// `objects` the object a number stands for (written by its members, never by its number). Never zeros for a key that is not the open one.
    pub(crate) fn intern(
        &mut self,
        data: KeyData,
        stable: impl Fn(DefId) -> StableDef,
        stable_var: impl Fn(VarId) -> CanonVar,
        list: impl Fn(Captures) -> CaptureList,
        objects: impl Fn(ObjectId) -> ObjectData,
    ) -> KeyId {
        let (index, new) = self.table.insert_full(data.normal());
        if new {
            let names = Names {
                stable: &stable,
                stable_var: &stable_var,
                list: &list,
                objects: &objects,
            };
            let made = digest(&CanonKey::of(&self.table[index], &names));
            #[cfg(debug_assertions)]
            {
                let fresh = self.seen.insert(made);
                debug_assert!(
                    fresh,
                    "two keys of one request with one digest: the canonical form lost a difference, and Graph::new would merge their contexts"
                );
            }
            self.digests.push(made);
        }
        KeyId(u32::try_from(index).expect(
            "a request has fewer than u32::MAX keys: its contexts are bounded by its budget",
        ))
    }

    /// What `key` holds, a key of this table.
    pub(crate) fn data(&self, key: KeyId) -> &KeyData {
        &self.table[key.0 as usize]
    }

    /// The digest of `key`, a key of this table.
    pub(crate) fn digest(&self, key: KeyId) -> [u8; 16] {
        self.digests[key.0 as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Keys::intern` for keys that hold no captured variable and no closure: the two resolvers are never asked.
    trait Plain {
        fn plain(&mut self, data: KeyData, stable: impl Fn(DefId) -> StableDef) -> KeyId;
    }

    impl Plain for Keys {
        fn plain(&mut self, data: KeyData, stable: impl Fn(DefId) -> StableDef) -> KeyId {
            self.intern(
                data,
                stable,
                |_| unreachable!("no variable is in these keys"),
                |_| unreachable!("no closure is in these keys"),
                |_| unreachable!("no object is in these keys"),
            )
        }
    }

    const ZEROS: [u8; 16] = [0; 16];
    const A: [u8; 16] = [
        93, 46, 87, 96, 216, 61, 155, 17, 211, 86, 34, 118, 74, 224, 161, 128,
    ];
    const B: [u8; 16] = [
        172, 21, 36, 62, 234, 108, 51, 149, 129, 38, 137, 144, 112, 95, 31, 68,
    ];
    const ANON: [u8; 16] = [
        227, 59, 111, 114, 81, 178, 31, 219, 156, 77, 31, 52, 186, 66, 183, 107,
    ];
    const CAPTURED: [u8; 16] = [
        148, 73, 235, 153, 58, 143, 27, 84, 69, 245, 212, 247, 104, 160, 122, 132,
    ];
    const CLOSURE: [u8; 16] = [
        206, 187, 220, 17, 90, 187, 122, 94, 215, 140, 199, 16, 16, 246, 50, 226,
    ];
    const A_OR_B: [u8; 16] = [
        55, 225, 1, 109, 241, 243, 47, 117, 120, 129, 24, 217, 191, 92, 235, 3,
    ];

    fn d(index: u32) -> DefId {
        DefId::new(0, 0, Some(index))
    }

    fn m(qname: &str) -> StableDef {
        StableDef::Path {
            module: "src/main.ts".into(),
            qname: qname.into(),
            nth: 0,
        }
    }

    /// `d0` is `a`, `d1` is `b`.
    fn by_name(def: DefId) -> StableDef {
        match def.index() {
            Some(0) => m("a"),
            Some(1) => m("b"),
            other => panic!("no name for {other:?}"),
        }
    }

    fn f(index: u32) -> Value {
        Value::of([One::Fn(d(index))], false)
    }

    /// A named parameter.
    fn param(capable: bool, rest: bool) -> Param {
        Param {
            rest,
            named: true,
            default: None,
            assigned: false,
            capable,
            uses: Default::default(),
        }
    }

    fn arg(index: u32, spread: bool) -> Arg {
        Arg {
            value: Expr::Def(d(index)),
            spread,
        }
    }

    /// What `value_of` answers by hand: `{Fn(d<i>)}` for `Def(d<i>)`, counting its calls.
    fn by_hand(calls: &mut u32) -> impl FnMut(&Expr) -> Value + '_ {
        move |expr| {
            *calls += 1;
            match expr {
                Expr::Def(def) => Value::of([One::Fn(*def)], false),
                other => panic!("not an argument of these tests: {other:?}"),
            }
        }
    }

    #[test]
    fn a_new_table_holds_the_open_key_alone_with_digest_zeros() {
        let keys = Keys::new();
        assert_eq!(keys.digest(KeyId::OPEN), ZEROS);
        assert_eq!(*keys.data(KeyId::OPEN), KeyData::default());
        assert_eq!(keys.table.len(), 1);
        assert_eq!(keys.digests.len(), 1);
    }

    #[test]
    fn interning_the_open_key_again_gives_its_number_and_adds_nothing() {
        let mut keys = Keys::new();
        let none = |_| unreachable!("no def is in the open key");
        assert_eq!(keys.plain(KeyData::default(), none), KeyId::OPEN);
        assert_eq!(keys.plain(KeyData::default(), none), KeyId::OPEN);
        assert_eq!(keys.table.len(), 1);
        assert_eq!(keys.digests.len(), 1);
    }

    #[test]
    fn trailing_unknown_parameters_are_dropped() {
        let normal = |params: Vec<Value>| {
            KeyData {
                params: params.into(),
                ..KeyData::default()
            }
            .normal()
            .params
        };
        assert_eq!(
            *normal(vec![f(0), Value::unknown(), Value::unknown()]),
            [f(0)]
        );
        assert_eq!(
            *normal(vec![Value::unknown(), f(0)]),
            [Value::unknown(), f(0)]
        );
        assert_eq!(
            *normal(vec![Value::data(), Value::unknown()]),
            [Value::data()]
        );
    }

    #[test]
    fn a_key_of_unknown_parameters_is_the_open_key() {
        let mut keys = Keys::new();
        let key = keys.plain(
            KeyData {
                params: vec![Value::unknown(), Value::unknown()].into(),
                ..KeyData::default()
            },
            |_| unreachable!(),
        );
        assert_eq!(key, KeyId::OPEN);
        assert_eq!(keys.table.len(), 1);
    }

    /// `[params]` as a `CanonKey`: its JSON and its digest.
    fn written(params: Vec<Canon>) -> (String, [u8; 16]) {
        let key = CanonKey {
            params,
            captured: vec![],
        };
        (serde_json::to_string(&key).unwrap(), digest(&key))
    }

    fn path(qname: &str) -> String {
        format!(r#"{{"Path":{{"module":"src/main.ts","qname":"{qname}","nth":0}}}}"#)
    }

    #[test]
    fn each_canonical_variant_has_its_pinned_bytes() {
        // a pin above that changes bumps this
        assert_eq!(CONTEXT_ID_VERSION, 1);
        let marked = || StableDef::Marked("abc".into());
        let rows: Vec<(Vec<Canon>, String, [u8; 16])> = vec![
            (
                vec![Canon::Unknown, Canon::Fn(m("a"))],
                format!(r#"{{"params":["Unknown",{{"Fn":{}}}]}}"#, path("a")),
                [
                    64, 110, 214, 97, 245, 134, 207, 177, 242, 253, 50, 72, 57, 215, 243, 92,
                ],
            ),
            (
                vec![Canon::Data],
                r#"{"params":["Data"]}"#.into(),
                [
                    110, 238, 15, 218, 35, 92, 78, 126, 227, 189, 64, 62, 10, 49, 239, 34,
                ],
            ),
            (
                vec![Canon::Wide],
                r#"{"params":["Wide"]}"#.into(),
                [
                    20, 189, 37, 208, 238, 14, 216, 41, 221, 109, 156, 169, 6, 147, 251, 26,
                ],
            ),
            (
                vec![Canon::Fn(m("a"))],
                format!(r#"{{"params":[{{"Fn":{}}}]}}"#, path("a")),
                A,
            ),
            (
                vec![Canon::Fn(m("b"))],
                format!(r#"{{"params":[{{"Fn":{}}}]}}"#, path("b")),
                B,
            ),
            (
                vec![Canon::Fn(m("handler"))],
                format!(r#"{{"params":[{{"Fn":{}}}]}}"#, path("handler")),
                [
                    63, 169, 7, 244, 195, 117, 63, 232, 189, 253, 184, 232, 224, 101, 115, 200,
                ],
            ),
            (
                vec![Canon::Fn(m("other"))],
                format!(r#"{{"params":[{{"Fn":{}}}]}}"#, path("other")),
                [
                    182, 165, 153, 246, 90, 12, 30, 220, 138, 89, 219, 215, 130, 124, 131, 66,
                ],
            ),
            (
                vec![Canon::Fn(marked())],
                r#"{"params":[{"Fn":{"Marked":"abc"}}]}"#.into(),
                [
                    100, 35, 32, 55, 78, 194, 123, 240, 12, 197, 116, 250, 72, 138, 154, 12,
                ],
            ),
            (
                vec![Canon::AnonArrow],
                r#"{"params":["AnonAny"]}"#.into(),
                ANON,
            ),
            (
                vec![Canon::AnonFunction],
                r#"{"params":["AnonFunction"]}"#.into(),
                [
                    121, 231, 11, 182, 209, 45, 66, 35, 206, 192, 213, 22, 171, 119, 195, 246,
                ],
            ),
            (
                vec![Canon::Undefined],
                r#"{"params":["Undefined"]}"#.into(),
                [
                    96, 15, 54, 81, 114, 34, 137, 20, 173, 89, 117, 222, 99, 2, 190, 64,
                ],
            ),
            (
                vec![Canon::Set {
                    known: vec![
                        Canon::Fn(marked()),
                        Canon::Fn(m("a")),
                        Canon::Fn(m("b")),
                        Canon::AnonArrow,
                        Canon::Undefined,
                    ],
                    open: true,
                }],
                format!(
                    r#"{{"params":[{{"Set":{{"known":[{{"Fn":{{"Marked":"abc"}}}},{{"Fn":{}}},{{"Fn":{}}},"AnonAny","Undefined"],"open":true}}}}]}}"#,
                    path("a"),
                    path("b")
                ),
                [
                    135, 6, 60, 121, 244, 220, 137, 157, 82, 115, 105, 68, 246, 185, 50, 78,
                ],
            ),
            (
                vec![Canon::Set {
                    known: vec![Canon::Fn(m("a")), Canon::Fn(m("b"))],
                    open: false,
                }],
                format!(
                    r#"{{"params":[{{"Set":{{"known":[{{"Fn":{}}},{{"Fn":{}}}],"open":false}}}}]}}"#,
                    path("a"),
                    path("b")
                ),
                A_OR_B,
            ),
            (
                vec![Canon::Set {
                    known: vec![Canon::Fn(m("a"))],
                    open: true,
                }],
                format!(
                    r#"{{"params":[{{"Set":{{"known":[{{"Fn":{}}}],"open":true}}}}]}}"#,
                    path("a")
                ),
                [
                    129, 19, 118, 224, 185, 190, 158, 152, 41, 71, 100, 254, 68, 70, 88, 56,
                ],
            ),
        ];
        for (params, json, expected) in rows {
            let (got_json, got_digest) = written(params.clone());
            assert_eq!(got_json, json, "{params:?}");
            assert_eq!(got_digest, expected, "{params:?}");
        }
    }

    #[test]
    fn a_value_is_written_in_one_canonical_form() {
        let names = Names {
            stable: &by_name,
            stable_var: &|_| unreachable!("no variable"),
            list: &|_| unreachable!("no closure"),
            objects: &|_| unreachable!("no object"),
        };
        let c = |value: Value| canon(&value, &names);
        let set = |known: Vec<Canon>, open| Canon::Set { known, open };
        assert_eq!(c(Value::unknown()), Canon::Unknown);
        assert_eq!(c(Value::data()), Canon::Data);
        assert_eq!(c(Value::wide()), Canon::Wide);
        assert_eq!(c(f(0)), Canon::Fn(m("a")));
        assert_eq!(
            c(Value::of([One::Fn(d(0))], true)),
            set(vec![Canon::Fn(m("a"))], true)
        );
        assert_eq!(c(Value::of([One::AnonArrow], false)), Canon::AnonArrow);
        assert_eq!(c(Value::of([One::Undefined], false)), Canon::Undefined);
        assert_eq!(
            c(Value::of([One::Fn(d(0)), One::Fn(d(1))], false)),
            set(vec![Canon::Fn(m("a")), Canon::Fn(m("b"))], false)
        );
    }

    #[test]
    fn a_set_is_written_in_canonical_order_not_in_def_order() {
        // `d0` is `b` and `d1` is `a`: the value lists `d0` first, its canonical form `a` first
        let swapped = |def: DefId| match def.index() {
            Some(0) => m("b"),
            Some(1) => m("a"),
            other => panic!("no name for {other:?}"),
        };
        let mut keys = Keys::new();
        let key = keys.plain(
            KeyData {
                params: vec![Value::of([One::Fn(d(0)), One::Fn(d(1))], false)].into(),
                ..KeyData::default()
            },
            swapped,
        );
        assert_eq!(keys.digest(key), A_OR_B);
    }

    #[test]
    fn anonymous_functions_are_one_member_in_a_key() {
        let mut keys = Keys::new();
        let none = |_| unreachable!("no def is in these keys");
        let anon = |def, n| One::Anon(d(def), n);
        let one = keys.plain(
            KeyData {
                params: vec![Value::of([anon(0, 0), anon(0, 1)], false)].into(),
                ..KeyData::default()
            },
            none,
        );
        let other = keys.plain(
            KeyData {
                params: vec![Value::of([anon(1, 3)], false)].into(),
                ..KeyData::default()
            },
            none,
        );
        assert_eq!(one, other);
        assert_eq!(keys.digest(one), ANON);
        let mixed = KeyData {
            params: vec![Value::of([anon(0, 0), One::Fn(d(1))], false)].into(),
            ..KeyData::default()
        }
        .normal();
        assert_eq!(
            *mixed.params,
            [Value::of([One::Fn(d(1)), One::AnonArrow], false)]
        );
    }

    #[test]
    fn arguments_bind_by_position() {
        let p = param(true, false);
        let mut calls = 0;
        let few = bind(
            &[p.clone(), p.clone()],
            &[arg(0, false)],
            &[true, true],
            by_hand(&mut calls),
        );
        assert_eq!(*few.params, [f(0), Value::of([One::Undefined], false)]);
        assert_eq!(calls, 1);
        let mut calls = 0;
        let many = bind(
            &[p.clone(), p.clone()],
            &[arg(0, false), arg(1, false), arg(2, false)],
            &[true, true],
            by_hand(&mut calls),
        );
        assert_eq!(*many.params, [f(0), f(1)]);
        assert_eq!(calls, 2, "the third argument is not read");
        let mut calls = 0;
        assert_eq!(
            bind(&[], &[arg(0, false)], &[], by_hand(&mut calls)),
            KeyData::default()
        );
        assert_eq!(calls, 0);
    }

    #[test]
    fn a_spread_makes_its_slot_and_every_later_one_unknown() {
        let p = param(true, false);
        let params = [p.clone(), p.clone(), p];
        let mut calls = 0;
        let mid = bind(
            &params,
            &[arg(0, false), arg(1, true), arg(2, false)],
            &[true; 3],
            by_hand(&mut calls),
        );
        assert_eq!(*mid.params, [f(0), Value::unknown(), Value::unknown()]);
        let all = bind(&params, &[arg(0, true)], &[true; 3], by_hand(&mut calls));
        assert_eq!(
            *all.params,
            [Value::unknown(), Value::unknown(), Value::unknown()]
        );
    }

    #[test]
    fn a_parameter_that_cannot_hold_a_value_is_unknown_and_its_argument_is_not_read() {
        let mut calls = 0;
        let key = bind(
            &[param(false, false), param(true, true)],
            &[arg(0, false), arg(1, false)],
            &[true, true],
            by_hand(&mut calls),
        );
        assert_eq!(*key.params, [Value::unknown(), Value::unknown()]);
        assert_eq!(calls, 0);
    }

    #[test]
    fn a_parameter_that_is_not_relevant_is_unknown_and_its_argument_is_not_read() {
        let p = param(true, false);
        let params = [p.clone(), p.clone(), p];
        let args = [arg(0, false), arg(1, false), arg(2, false)];
        let mut calls = 0;
        let key = bind(&params, &args, &[true, false, true], by_hand(&mut calls));
        assert_eq!(*key.params, [f(0), Value::unknown(), f(2)]);
        assert_eq!(calls, 2);
        // a flag the slice does not have is not relevant either: the callers pass a slice as long as the parameters
        let mut calls = 0;
        let key = bind(&params, &args, &[true], by_hand(&mut calls));
        assert_eq!(*key.params, [f(0), Value::unknown(), Value::unknown()]);
        assert_eq!(calls, 1);
    }

    #[test]
    fn interning_makes_one_digest_per_key() {
        let mut keys = Keys::new();
        assert_eq!(keys.digest(KeyId::OPEN), ZEROS);
        let key = |def: u32| KeyData {
            params: vec![f(def)].into(),
            ..KeyData::default()
        };
        let a = keys.plain(key(0), by_name);
        assert_eq!(a, KeyId(1));
        assert_eq!(keys.digest(a), A);
        assert_eq!(keys.plain(key(0), by_name), a);
        let b = keys.plain(key(1), by_name);
        assert_eq!(b, KeyId(2));
        assert_eq!(keys.digest(b), B);
        assert_eq!(keys.table.len(), 3);
        assert_eq!(keys.digests.len(), 3);
    }

    /// A captured variable of `start`'s, named by hand.
    fn on_done() -> CanonVar {
        CanonVar {
            def: m("start"),
            name: "onDone".into(),
            nth: 0,
        }
    }

    fn written_with(params: Vec<Canon>, captured: Vec<(CanonVar, Canon)>) -> (String, [u8; 16]) {
        let key = CanonKey { params, captured };
        (serde_json::to_string(&key).unwrap(), digest(&key))
    }

    fn var(def: u32, at: u32) -> VarId {
        VarId { def: d(def), at }
    }

    #[test]
    fn a_captured_variable_and_a_closure_have_their_pinned_bytes() {
        let var_json = format!(r#"{{"def":{},"name":"onDone","nth":0}}"#, path("start"));
        // a key that holds a captured variable and no parameter
        let (json, got) = written_with(vec![], vec![(on_done(), Canon::Fn(m("save")))]);
        assert_eq!(
            json,
            format!(
                r#"{{"params":[],"captured":[[{var_json},{{"Fn":{}}}]]}}"#,
                path("save")
            )
        );
        assert_eq!(got, CAPTURED, "{got:?}");
        // a slot that holds a closure: the function and what it captured
        let closure = Canon::Closure {
            func: m("finish"),
            captured: vec![(on_done(), Canon::Fn(m("save")))],
        };
        let (json, got) = written_with(vec![closure], vec![]);
        assert_eq!(
            json,
            format!(
                r#"{{"params":[{{"Closure":{{"func":{},"captured":[[{var_json},{{"Fn":{}}}]]}}}}]}}"#,
                path("finish"),
                path("save")
            )
        );
        assert_eq!(got, CLOSURE, "{got:?}");
        // a key with no captured variable is written without the field, so its digest is the one it had
        assert_eq!(written_with(vec![Canon::Fn(m("a"))], vec![]).1, A);
    }

    /// An object whose one member `onDone` holds the function `save`, in its canonical form.
    fn save_object() -> Canon {
        Canon::Object {
            members: vec![("onDone".into(), Canon::Fn(m("save")))],
            complete: true,
        }
    }

    const OBJECT: [u8; 16] = [
        26, 10, 101, 238, 90, 107, 139, 9, 120, 177, 209, 107, 114, 254, 170, 117,
    ];

    #[test]
    fn an_object_has_its_pinned_bytes() {
        let (json, got) = written_with(vec![save_object()], vec![]);
        assert_eq!(
            json,
            format!(
                r#"{{"params":[{{"Object":{{"members":[["onDone",{{"Fn":{}}}]],"complete":true}}}}]}}"#,
                path("save")
            )
        );
        assert_eq!(got, OBJECT, "{got:?}");
        // an object that is not complete is another key
        let open = Canon::Object {
            members: vec![("onDone".into(), Canon::Fn(m("save")))],
            complete: false,
        };
        assert_ne!(written_with(vec![open], vec![]).1, got);
    }

    #[test]
    fn an_objects_digest_does_not_follow_its_number() {
        use crate::nested::Nested;
        let save = |def: u32| vec![("onDone".into(), f(def))];
        let key_of = |nested: &mut Nested, first: u32, second: u32| {
            // the two objects are made in this order, so each has the other's number in the other table
            let (one, _) = nested.intern_object(save(first), true);
            let (other, _) = nested.intern_object(save(second), true);
            (one.unwrap(), other.unwrap())
        };
        let mut forward = Nested::new();
        let (a_forward, _) = key_of(&mut forward, 0, 1);
        let mut backward = Nested::new();
        let (_, a_backward) = key_of(&mut backward, 1, 0);
        // the same object, `{ onDone: a }`, has two numbers
        assert_ne!(a_forward, a_backward);
        let digest_of = |nested: &Nested, id: ObjectId| {
            let mut keys = Keys::new();
            let key = keys.intern(
                KeyData {
                    params: vec![Value::of([One::Object(id)], false)].into(),
                    ..KeyData::default()
                },
                by_name,
                |_| unreachable!("no variable"),
                |_| unreachable!("no closure"),
                |id| nested.object(id).clone(),
            );
            keys.digest(key)
        };
        let (one, other) = (
            digest_of(&forward, a_forward),
            digest_of(&backward, a_backward),
        );
        assert_eq!(one, other);
        assert_ne!(one, ZEROS);
    }

    #[test]
    fn the_captured_entries_are_in_their_normal_form() {
        let normal = |captured: Vec<(VarId, Value)>| {
            KeyData {
                params: Box::new([]),
                captured: captured.into(),
            }
            .normal()
            .captured
        };
        let arrow = Value::of([One::Anon(d(4), 1), One::Fn(d(1))], false);
        assert_eq!(
            *normal(vec![
                (var(2, 9), f(0)),
                (var(1, 5), Value::unknown()),
                (var(1, 3), arrow),
            ]),
            [
                (var(1, 3), Value::of([One::Fn(d(1)), One::AnonArrow], false)),
                (var(2, 9), f(0)),
            ]
        );
        // a key whose only content was unknown is the open key
        let mut keys = Keys::new();
        let open = keys.plain(
            KeyData {
                params: Box::new([]),
                captured: vec![(var(1, 5), Value::unknown())].into(),
            },
            |_| unreachable!(),
        );
        assert_eq!(open, KeyId::OPEN);
    }

    #[test]
    fn a_captured_variable_makes_a_key_of_its_own_and_its_digest_does_not_follow_the_order_of_defs()
    {
        let captured_name = |var: VarId| CanonVar {
            def: m("start"),
            name: format!("v{}", var.at),
            nth: 0,
        };
        let none = |_: Captures| -> CaptureList { unreachable!() };
        let no_object = |_: ObjectId| -> ObjectData { unreachable!() };
        let mut keys = Keys::new();
        let one = KeyData {
            params: Box::new([]),
            captured: vec![(var(1, 3), f(0)), (var(2, 9), f(1))].into(),
        };
        let a = keys.intern(one.clone(), by_name, captured_name, none, no_object);
        assert_ne!(a, KeyId::OPEN);
        assert_eq!(keys.intern(one, by_name, captured_name, none, no_object), a);
        // the same two variables under another numbering of defs: `v9`'s def now comes before `v3`'s, so the list is in the other order, and the digest is
        // the same, since the list is written in the order of its canonical form
        let mut other = Keys::new();
        let swapped = KeyData {
            params: Box::new([]),
            captured: vec![(var(1, 9), f(1)), (var(2, 3), f(0))].into(),
        };
        let b = other.intern(swapped, by_name, captured_name, none, no_object);
        assert_eq!(keys.digest(a), other.digest(b));
        assert_ne!(keys.digest(a), ZEROS);
    }
}
