//! Evaluating a body (`docs/08-bend/07-value-layer-design.md` §5): under `Precision::Precise` a def is walked by reading its `Flow` under its key, and the answer is,
//! for each item of the def, what it comes to (`Reached`), the same type the item walk of layer 1 gives. The evaluator decides the **targets** of items and never
//! invents a site: every site is an item (BV-D2), which is what makes "it knows nothing, so it is layer 1" a test that can be run item by item.
//!
//! What carries a value: a parameter has the one the key gives it, a def named in an argument is that def, a lambda is a lambda that **stays** (its body is read
//! here, where it is written, its binders unknown), a variable has the value of its binding (BV-D3: a `let` gives its names the values of its terms, a `case` with a
//! name pattern gives it the value of the scrutinee), and, since V6, a call that gives a def all its written parameters has what that def's body evaluates to under
//! the key those arguments make (`ret`, BV-D5); a call that gives a def fewer is a **partial application**, a value that is handed over where it is written and called
//! where the rest arrive (07 §7); and a `do` statement of a project monad is a call to the monad's `bind` or `pure`, with its value at its place in the key (07 §8).
//! A constructor of a project type has its fields' values, and a pattern of that constructor binds its names to them (07 §9). Every other binder has none.
//!
//! 🦀 The evaluator is a struct that borrows the book, the key's environment and the def's tree, and owns the one thing it fills in: a `Vec<Option<Reached>>`, one
//! place for each item. `Option` because an item that is not a node (a type skipped, Base's `&&`) is not the evaluator's: the walk that calls it settles those.
//!
//! 🦀 `ret` asks the question "what does this context return" of a context that may be asking it of itself, so its memo has three states for a key: absent, `Running`
//! (with the value so far) and `Done`. The memo sits in a `RefCell` on `Bend` so the evaluator can stay `&self`; the borrow is dropped before the evaluator is run,
//! or the inner call would panic on a second `borrow_mut`.

use std::collections::HashMap;

use analyzer_core::graph::Reason;
use analyzer_core::solve::Target;

use crate::book::{DeclId, Resolved};
use crate::calls::{self, Bend, Event};
use crate::env::Env;
use crate::key::{self, Absent, Key};
use crate::probe::{self, Section};
use crate::sem::{ArgShape, DefWalk, Expr, Item, Pat, Sugar, What};
use crate::value::{One, Value};
use crate::walk::{self, Reached};

/// A cell evaluated more often than this makes every cell of its request unknown (07 §7): a backstop far above anything real, since the caps on a value (a set of at
/// most `MAX_SET` members nested at most `MAX_DEPTH` deep) are what make the worklist end.
pub(crate) const MAX_EVALS: u32 = 64;

/// What the evaluator may know. `Tracked` is the layer. `Nothing` is for the test that proves the evaluator and the item table agree: every value is unknown, so
/// the answer must be layer 1's, site by site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Policy {
    Tracked,
    #[cfg(test)]
    Nothing,
}

/// The way a request is worked out. Only the tests choose: they build every graph the old way too, and the two must be byte-identical (V10b, V10d).
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ways {
    /// The sites of the last evaluation of a context are kept for the solver's walk (V10b).
    pub(crate) keep: bool,
}

#[cfg(test)]
thread_local! {
    static WAYS: std::cell::Cell<Ways> = const { std::cell::Cell::new(Ways { keep: true }) };
}

/// Runs `then` with the request worked out in `ways`, on this thread.
#[cfg(test)]
pub(crate) fn with_ways<T>(ways: Ways, then: impl FnOnce() -> T) -> T {
    let before = WAYS.replace(ways);
    let out = then();
    WAYS.set(before);
    out
}

fn keeps() -> bool {
    #[cfg(test)]
    return WAYS.get().keep;
    #[cfg(not(test))]
    true
}

/// What one evaluation of a context gave besides its value: what each item of its body came to, and what reading it counted. Kept until the solver walks the context,
/// when it is the walk's own reading (V10b): the evaluation is the one `reached` makes, so the sites are the same.
pub(crate) struct Kept<'b> {
    reached: Vec<Option<Reached<'b>>>,
    events: Vec<Event>,
}

struct Eval<'l, 'b> {
    lang: &'l Bend<'b>,
    def: DeclId,
    env: &'l Env<'l>,
    walk: &'b DefWalk,
    /// One place for each item of the def, filled in by the node that carries it.
    reached: Vec<Option<Reached<'b>>>,
    /// The value of each binder that is not a parameter, by number: set where it is bound, which is before any read of it (a binder is in scope only below its
    /// binding). A number never set is unknown.
    vars: Vec<Value>,
}

impl<'l, 'b> Eval<'l, 'b> {
    fn new(lang: &'l Bend<'b>, def: DeclId, env: &'l Env<'l>, walk: &'b DefWalk) -> Self {
        Eval {
            lang,
            def,
            env,
            walk,
            reached: (0..walk.items.len()).map(|_| None).collect(),
            vars: Vec::new(),
        }
    }
}

/// What each item of `f` comes to under `env`, by item: `Some` for every item that is a site and for every name (handed over, or below the arguments), `None` for the
/// rest, which the item walk settles. Empty when `f` has no walk (not a def of the book).
pub(crate) fn reached<'b>(lang: &Bend<'b>, f: DeclId, env: &Env<'_>) -> Vec<Option<Reached<'b>>> {
    if let Some(kept) = lang.kept.borrow_mut().remove(&(f, env.key().clone())) {
        lang.replay(&kept.events);
        probe::kept_used();
        return kept.reached;
    }
    let Some(walk) = lang.walk_of(f) else {
        return Vec::new();
    };
    let mut eval = Eval::new(lang, f, env, walk);
    eval.eval(&walk.flow.body);
    eval.reached
}

/// The contexts whose return value is being worked out, together: one request of `ret` and everything it needs that is not known yet. A context is a `(def, key)`
/// and is numbered when it is first met.
///
/// The value a context returns starts as data, "nothing yet". A context that is read while the values are being worked out answers with its value so far; when a value
/// grows, every context that read it is evaluated again, until none grows. That is the least value each context can have, whichever was met first (BV-D5), and it is the
/// whole of "nothing computed inside a cycle is kept before that": no value is kept until all of them are final. A context met for the first time is evaluated before
/// the one that asked, so code with no cycle is evaluated once.
#[derive(Default)]
pub(crate) struct Run<'b> {
    index: HashMap<Ctx, usize>,
    ctxs: Vec<Ctx>,
    cells: Vec<Cell<'b>>,
    /// Contexts to evaluate, the last first.
    work: Vec<usize>,
    queued: Vec<bool>,
    /// The context being evaluated: what a read is a read by.
    reading: Option<usize>,
}

type Ctx = (DeclId, Key);

struct Cell<'b> {
    value: Value,
    /// The contexts that read this one, in the order they first did.
    readers: Vec<usize>,
    /// How many times it was evaluated.
    evals: u32,
    /// Its value was too big (a set past the cap): it is unknown, and stays so.
    top: bool,
    /// What its last evaluation gave besides the value.
    kept: Option<Kept<'b>>,
}

impl<'b> Run<'b> {
    fn cell_for(&mut self, ctx: Ctx) -> usize {
        if let Some(at) = self.index.get(&ctx) {
            return *at;
        }
        let at = self.cells.len();
        self.index.insert(ctx.clone(), at);
        self.ctxs.push(ctx);
        self.cells.push(Cell {
            value: Value::data(),
            readers: Vec::new(),
            evals: 0,
            top: false,
            kept: None,
        });
        self.queued.push(true);
        self.work.push(at);
        at
    }

    /// The value of `at` so far, read by the context being evaluated.
    fn read(&mut self, at: usize) -> Value {
        if let Some(reader) = self.reading
            && !self.cells[at].readers.contains(&reader)
        {
            self.cells[at].readers.push(reader);
        }
        self.cells[at].value.clone()
    }

    /// `read` is what `at` was just evaluated to: its value grows to hold it, and what read it is evaluated again if it grew. Returns what to count: `(a set too big,
    /// a cell evaluated more than `MAX_EVALS` times)`.
    fn settle(&mut self, at: usize, read: Value) -> (bool, bool) {
        self.reading = None;
        self.cells[at].evals += 1;
        if self.cells[at].evals > MAX_EVALS {
            return (false, true);
        }
        if self.cells[at].top {
            return (false, false);
        }
        let grown = self.cells[at].value.join(&read).unwrap_or_else(Value::top);
        // a cell that becomes the top is counted, whether the set that was too big was its own or one it read (what a body counts is kept with its sites, and counted when its context is walked, but this is counted once, here)
        let big = grown.is_top() && !self.cells[at].value.is_top();
        if grown == self.cells[at].value {
            return (big, false);
        }
        let cell = &mut self.cells[at];
        cell.top = grown.is_top();
        let from_data = !cell.value.is_open() && cell.value.members().is_empty();
        cell.value = grown;
        let mut queued = 0;
        for reader in cell.readers.clone() {
            if !std::mem::replace(&mut self.queued[reader], true) {
                self.work.push(reader);
                queued += 1;
            }
        }
        probe::requeued(from_data, queued);
        (big, false)
    }

    /// Every cell of the request is unknown, and nothing is left to evaluate.
    fn give_up(&mut self) {
        for cell in &mut self.cells {
            cell.value = Value::top();
            cell.top = true;
        }
        self.work.clear();
    }
}

/// What the body of `def` evaluates to under `key`, read once: no site is kept and nothing is counted.
fn read_value<'b>(
    lang: &Bend<'b>,
    def: DeclId,
    key: &Key,
    again: bool,
) -> (Value, Option<Kept<'b>>) {
    probe::value_read(def, key, again);
    let _time = probe::section(Section::Value);
    let Some(walk) = lang.walk_of(def) else {
        return (Value::unknown(), None);
    };
    let env = Env::new(key);
    // what the body counts is not counted now: it is kept with its sites, and counted when the solver walks the context
    let ((value, reached), events) = lang.recording(|| {
        let mut eval = Eval::new(lang, def, &env, walk);
        let value = eval.eval(&walk.flow.body);
        (value, eval.reached)
    });
    (value, keeps().then_some(Kept { reached, events }))
}

/// The value a context returns (BV-D5), kept: `(def, key)` is worked out once, with everything it needs (`Run`). Asked while a request is being worked out, a context
/// answers with its value so far, which starts as data.
pub(crate) fn ret(lang: &Bend<'_>, def: DeclId, key: &Key) -> Value {
    let ctx = (def, key.clone());
    if let Some(value) = lang.rets.borrow().get(&ctx) {
        return value.clone();
    }
    if let Some(run) = lang.run.borrow_mut().as_mut() {
        let at = run.cell_for(ctx);
        return run.read(at);
    }
    // not inside a request: this is the first, and the others come from it
    let _time = probe::section(Section::Worklist);
    let root = {
        let mut run = Run::default();
        let root = run.cell_for(ctx);
        *lang.run.borrow_mut() = Some(run);
        root
    };
    let mut gave_up = false;
    loop {
        let next = {
            let mut guard = lang.run.borrow_mut();
            let run = guard.as_mut().expect("a request is being worked out");
            run.work.pop().inspect(|at| {
                run.queued[*at] = false;
                run.reading = Some(*at);
            })
        };
        let Some(at) = next else { break };
        let again = lang.run.borrow().as_ref().expect("a request").cells[at].evals > 0;
        let (def, key) = {
            let guard = lang.run.borrow();
            guard.as_ref().expect("a request").ctxs[at].clone()
        };
        let (read, kept) = read_value(lang, def, &key, again);
        let (big, cycle) = {
            let mut guard = lang.run.borrow_mut();
            let run = guard.as_mut().expect("a request");
            run.cells[at].kept = kept;
            run.settle(at, read)
        };
        if big {
            lang.note_once(calls::VALUE_WIDENED_SET);
        }
        if cycle {
            lang.note_once(calls::RETURN_IN_CYCLE);
            lang.run.borrow_mut().as_mut().expect("a request").give_up();
            gave_up = true;
        }
    }
    let run = lang.run.borrow_mut().take().expect("a request");
    let value = run.cells[root].value.clone();
    let mut rets = lang.rets.borrow_mut();
    let mut kept = lang.kept.borrow_mut();
    for (ctx, cell) in run.ctxs.into_iter().zip(run.cells) {
        // the last evaluation of a context read the final value of everything it read, unless the request gave up: then every value is the top, and the walk reads
        if !gave_up && let Some(evaluated) = cell.kept {
            kept.insert(ctx.clone(), evaluated);
        }
        rets.insert(ctx, cell.value);
    }
    value
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
            // a parameter has what the key says; any other binder has the value of its binding (BV-D3)
            Expr::Var(n) if *n < self.walk.flow.params => {
                let value = self.env.known(*n).cloned().unwrap_or_else(Value::unknown);
                self.known(value)
            }
            Expr::Var(n) => self
                .vars
                .get(*n as usize)
                .cloned()
                .unwrap_or_else(Value::unknown),
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
                // a callee that is a variable or a call's result is called as what it holds; any other callee that is not a name (`(x => ..)(a)`) is not followed
                let follows = matches!(&**head, Expr::Var(_) | Expr::Call { .. });
                let (reached, value) = self.call(*item, follows.then_some(&head_value), &values);
                self.reached[*item as usize] = Some(reached);
                value
            }
            Expr::Ctr { item, name, fields } => {
                let held: Vec<Value> = fields.iter().map(|f| self.eval(f)).collect();
                self.settle(*item);
                self.object(name, held)
            }
            Expr::Sugar { item, operands } => {
                operands.iter().for_each(|o| drop(self.eval(o)));
                self.settle(*item);
                Value::data()
            }
            // a statement of a `do` is a call to the monad's `bind` or `pure`; its continuation stays, read here
            Expr::Bind {
                item,
                types,
                value,
                rest,
                pure,
                ..
            } => {
                let held = self.eval(value);
                self.eval(rest);
                let (reached, value) = self.statement(*item, types.len(), value, held, *pure);
                self.reached[*item as usize] = Some(reached);
                value
            }
            // the values are read before the names bind (`x = f(x)` reads the outer `x`), and each name takes the value of its term
            Expr::Let { pats, values, rest } => {
                let values: Vec<Value> = values.iter().map(|v| self.eval(v)).collect();
                self.bind_all(pats, &values);
                self.eval(rest)
            }
            // no condition is evaluated and no case is ruled out: the value is the join of the cases
            Expr::Match { on, cases } => {
                let on: Vec<Value> = on.iter().map(|e| self.eval(e)).collect();
                let mut joined: Option<Value> = None;
                for (pats, body) in cases.iter() {
                    self.bind_all(pats, &on);
                    let value = self.eval(body);
                    joined = Some(match joined {
                        None => value,
                        Some(so_far) => self.join(&so_far, &value),
                    });
                }
                joined.unwrap_or_else(Value::data)
            }
            Expr::Seq(parts) => parts
                .iter()
                .map(|p| self.eval(p))
                .last()
                .unwrap_or_else(Value::data),
        }
    }

    /// What can be at a place that holds `a` in one case and `b` in another; a set past the cap is the top, and counted.
    fn join(&self, a: &Value, b: &Value) -> Value {
        a.join(b).unwrap_or_else(|| {
            self.lang.note_slot(calls::VALUE_WIDENED_SET);
            Value::top()
        })
    }

    /// The patterns of a `let` or a `case` against the values they match, one for one. When the counts differ nothing is bound.
    fn bind_all(&mut self, pats: &[Pat], values: &[Value]) {
        if pats.len() != values.len() {
            return;
        }
        for (pat, value) in pats.iter().zip(values) {
            self.bind(pat, value);
        }
    }

    /// A pattern against a value. A name takes the value. A constructor takes each field from the join of that field over the members of the value that are that
    /// constructor, and from nothing when the value is open or has no such member (no case is ruled out, so a case that cannot match still binds, to unknown). A list, a
    /// tuple and the rest bind their names to nothing the layer can say (a binder never set is unknown).
    fn bind(&mut self, pat: &Pat, value: &Value) {
        match pat {
            Pat::Bind(n) => {
                let at = *n as usize;
                if self.vars.len() <= at {
                    self.vars.resize(at + 1, Value::unknown());
                }
                self.vars[at] = value.clone();
            }
            Pat::Ctr { name, fields } => {
                let held = self.fields_of(name, value, fields.len());
                for (field, held) in fields.iter().zip(held) {
                    self.bind(field, &held);
                }
            }
            Pat::Opaque(_) | Pat::Skip => {}
        }
    }

    /// The constructor `name` of a project type, as the book names it: its type, and the name without the path it was written with.
    fn ctor_of(&self, name: &str) -> Option<(DeclId, String)> {
        match self.lang.book.ctor(self.def.file(), name) {
            Resolved::Project(ty) => Some((ty, name.rsplit('.').next().unwrap_or(name).to_owned())),
            _ => None,
        }
    }

    /// A constructor with what it was given, as a value; nested deeper than the cap, it is unknown there, and counted. A constructor of Base's is unknown: what
    /// Base does with it is not ours to read. One of a project type with no function anywhere inside it is data: nothing a call can use is there, so it is in no key,
    /// and "nothing yet" stays "nothing yet" (07 §7).
    fn object(&self, name: &str, held: Vec<Value>) -> Value {
        let Some((ty, written)) = self.ctor_of(name) else {
            return Value::unknown();
        };
        if !self.tracks() {
            return Value::unknown();
        }
        if !held.iter().any(Value::holds_function) {
            return Value::data();
        }
        let (value, hit) = Value::one(One::Ctor {
            ty,
            name: written.into(),
            fields: held.into(),
        })
        .capped();
        if hit {
            self.lang.note_slot(calls::VALUE_WIDENED_DEPTH);
        }
        value
    }

    /// What each of `count` fields of the constructor `name` holds in `value`: the join over its members that are that constructor, and unknown beside them when the
    /// value is open (something else may be there too). With no such member it is data when the value is closed ("nothing yet" stays "nothing yet"), and unknown when
    /// it is open.
    fn fields_of(&self, name: &str, value: &Value, count: usize) -> Vec<Value> {
        let unknown = || vec![Value::unknown(); count];
        let Some((ty, written)) = self.ctor_of(name) else {
            return unknown();
        };
        // what is too much for the caps has nothing to take apart, and is above everything
        if value.is_top() {
            return vec![Value::top(); count];
        }
        let mut joined: Option<Vec<Value>> = None;
        for one in value.members() {
            let One::Ctor {
                ty: of,
                name: made,
                fields,
            } = one
            else {
                continue;
            };
            if *of != ty || **made != *written {
                continue;
            }
            if fields.len() != count {
                return unknown();
            }
            joined = Some(match joined {
                None => fields.to_vec(),
                Some(so_far) => so_far
                    .iter()
                    .zip(fields.iter())
                    .map(|(a, b)| self.join(a, b))
                    .collect(),
            });
        }
        match joined {
            // the members' fields, and something else beside them when the value is open
            Some(fields) if value.is_open() => fields
                .iter()
                .map(|field| self.join(field, &Value::unknown()))
                .collect(),
            Some(fields) => fields,
            None if value.is_open() => unknown(),
            None => vec![Value::data(); count],
        }
    }

    /// An item that no value changes the answer for: the book's, as layer 1 gives it.
    fn settle(&mut self, at: u32) {
        let reached = walk::resolve_from_book(self.lang, self.def, self.item(at), self.env);
        self.reached[at as usize] = Some(reached);
    }

    /// Whether the evaluator may know what it learns: the policy that takes it all away is the test of layer 1.
    fn tracks(&self) -> bool {
        self.lang.policy == Policy::Tracked
    }

    /// How many parameters a def with code has written.
    fn params_of(&self, def: DeclId) -> Option<usize> {
        if !key::known_def(self.lang, def) {
            return None;
        }
        self.lang.walk_of(def).map(|w| w.flow.params as usize)
    }

    /// A def and what it was given, as a value: nested deeper than the cap, it is unknown there, and counted.
    fn partial(&self, def: DeclId, args: Vec<Value>) -> Value {
        let (value, hit) = Value::one(One::Partial {
            def,
            args: args.into(),
        })
        .capped();
        if hit {
            self.lang.note_slot(calls::VALUE_WIDENED_DEPTH);
        }
        value
    }

    /// What a `do` statement of the monad `M` comes to (07 §8): the call `M.bind(T.., A, B, v, k)` (or `M.pure(T.., B, e)`), whose statement's value `held` is at the
    /// place after the type terms. The continuation `k` is never in the key, so every statement that binds the same value is one context of `bind`.
    fn statement(
        &self,
        at: u32,
        types: usize,
        value: &Expr,
        held: Value,
        pure: bool,
    ) -> (Reached<'b>, Value) {
        let item = self.item(at);
        let book_only = (
            walk::resolve_from_book(self.lang, self.def, item, self.env),
            Value::unknown(),
        );
        let What::Sugar(Sugar::Named(written)) = &item.what else {
            return book_only;
        };
        let Resolved::Project(callee) = self.lang.book.resolve(self.def.file(), written) else {
            return book_only;
        };
        if !self.tracks() {
            return book_only;
        }
        let shape = self.shape_of(value);
        let key = key::key_with(self.lang, callee, |position| {
            let position = position as usize;
            if position == types {
                self.slot(Some(&shape), Some(&held))
            } else if position < types {
                Err(Absent::Value)
            } else {
                Err(Absent::Stays)
            }
        });
        let given = types + if pure { 1 } else { 2 };
        let returned = self.returned(callee, &key, given);
        let reached = walk::site(
            walk::call_of(self.lang, Resolved::Project(callee), written, key),
            false,
        );
        (reached, returned)
    }

    /// What an expression is as an argument, for saying why it has no slot: the text's own classification (`ArgShape`), read off the tree.
    fn shape_of(&self, expr: &Expr) -> ArgShape {
        match expr {
            Expr::Name { name, .. } => ArgShape::Name(name.clone()),
            Expr::Var(n) if *n < self.walk.flow.params => ArgShape::Param(*n),
            Expr::Var(_) => ArgShape::Local,
            Expr::Lam { nth, .. } => ArgShape::Lambda(*nth),
            _ => ArgShape::Other,
        }
    }

    /// What a def returns when it is given `given` arguments under `key`: its body's value when they are all of its written parameters, and unknown otherwise.
    fn returned(&self, callee: DeclId, key: &Key, given: usize) -> Value {
        let all = self
            .lang
            .walk_of(callee)
            .is_some_and(|w| w.flow.params as usize == given);
        if all && key::known_def(self.lang, callee) {
            ret(self.lang, callee, key)
        } else {
            Value::unknown()
        }
    }

    /// What a call comes to, and the value it has. Its callee is a name the book resolves, or a value: `head` is the value of the callee when the callee is a variable
    /// or a call's result, `values` those of the arguments.
    fn call(&self, at: u32, head: Option<&Value>, values: &[Value]) -> (Reached<'b>, Value) {
        let item = self.item(at);
        match (&item.what, head) {
            // an unbound name: the book fixes what it is (a name it fixes is never overridden), and the key is made of the arguments
            (What::Call { name, args }, _) => {
                let resolved = self.lang.book.resolve(self.def.file(), name);
                match &resolved {
                    Resolved::Project(id) => {
                        // fewer arguments than written parameters: the def is handed over here, its body does not run, and the rest are given where it is called
                        if self.tracks() && self.params_of(*id).is_some_and(|n| values.len() < n) {
                            let reached =
                                walk::site(walk::project(self.lang, *id, Key::open()), true);
                            return (reached, self.partial(*id, values.to_vec()));
                        }
                        let key = self.key(*id, args, values);
                        let value = self.returned(*id, &key, values.len());
                        let reached =
                            walk::site(walk::call_of(self.lang, resolved, name, key), false);
                        (reached, value)
                    }
                    _ => (
                        walk::site(walk::call_of(self.lang, resolved, name, Key::open()), false),
                        Value::unknown(),
                    ),
                }
            }
            // a call through a value: one answer for each member of what it holds, and the book's own beside them when something else may be there
            (What::Opaque { args, .. }, Some(head)) => self.apply(head, args, values),
            _ => (
                walk::resolve_from_book(self.lang, self.def, item, self.env),
                Value::unknown(),
            ),
        }
    }

    /// What applying `head` to `values` comes to, the arguments written as `shapes`.
    fn apply(&self, head: &Value, shapes: &[ArgShape], values: &[Value]) -> (Reached<'b>, Value) {
        let mut targets = Vec::new();
        // a partial application that is still short of its arguments after these: handed over again, if nothing else is called
        let mut short = Vec::new();
        let mut value: Option<Value> = None;
        let mut lambda = false;
        // a constructor is not something to call: the book's answer stands beside whatever else is called
        let mut object = false;
        let mut add = |this: &Self, v: Value| {
            value = Some(match value.take() {
                None => v,
                Some(so_far) => this.join(&so_far, &v),
            });
        };
        for one in head.members() {
            match one {
                One::Def(callee) => {
                    let key = self.key(*callee, shapes, values);
                    add(self, self.returned(*callee, &key, values.len()));
                    targets.push(walk::project(self.lang, *callee, key));
                }
                // known, and drawn where it is written: one answer however many lambdas the variable may hold
                One::Lambda { .. } => {
                    add(self, Value::unknown());
                    if !lambda {
                        lambda = true;
                        targets.push(Target::Unresolved(Reason::new(
                            calls::ANONYMOUS_FUNCTION_CALLED,
                        )));
                    }
                }
                // not callable, and nothing is what calling it returns
                One::Ctor { .. } => {
                    add(self, Value::data());
                    object = true;
                }
                // the arguments it was given, and these: all of them is the call, fewer is a partial application again
                One::Partial { def, args } => {
                    let all: Vec<Value> = args.iter().chain(values).cloned().collect();
                    let held: Vec<ArgShape> = args
                        .iter()
                        .map(|_| ArgShape::Other)
                        .chain(shapes.iter().cloned())
                        .collect();
                    let n = self.params_of(*def).unwrap_or(0);
                    if all.len() < n {
                        add(self, self.partial(*def, all));
                        short.push(*def);
                    } else {
                        let key = self.key(*def, &held, &all);
                        add(self, self.returned(*def, &key, all.len()));
                        targets.push(walk::project(self.lang, *def, key));
                    }
                }
            }
        }
        let passed = targets.is_empty() && !short.is_empty() && !head.is_open();
        if passed {
            targets = short
                .iter()
                .map(|def| walk::project(self.lang, *def, Key::open()))
                .collect();
        } else if head.is_open() || object || targets.is_empty() {
            targets.push(Target::Unresolved(Reason::new(calls::FUNCTION_TYPED_VALUE)));
            // calling what is open may return anything; calling what has nothing callable in it returns nothing
            add(
                self,
                if head.is_open() {
                    Value::unknown()
                } else {
                    Value::data()
                },
            );
        }
        (
            Reached::Site { targets, passed },
            value.unwrap_or_else(Value::data),
        )
    }

    /// The key of a call of `callee`, from the values of its arguments (06 §4).
    fn key(&self, callee: DeclId, shapes: &[ArgShape], values: &[Value]) -> Key {
        key::key_with(self.lang, callee, |at| {
            self.slot(shapes.get(at as usize), values.get(at as usize))
        })
    }

    /// What the argument at a position puts in a slot: its value, whatever the argument is, when that is neither unknown nor data. A set is a value, and a slot may hold
    /// one. When there is nothing to put, `shape` (what the text says the argument is) says why, and it is counted as it always was.
    fn slot(&self, shape: Option<&ArgShape>, value: Option<&Value>) -> Result<Value, Absent> {
        let Some(shape) = shape else {
            return Err(Absent::Missing);
        };
        match value {
            Some(value) if !value.members().is_empty() => Ok(value.clone()),
            _ => Err(match shape {
                ArgShape::Param(_) => Absent::Open,
                ArgShape::Local => Absent::Local,
                ArgShape::Name(name) => key::absent_name(self.lang, self.def, name),
                ArgShape::Lambda(_) | ArgShape::Other => Absent::Value,
            }),
        }
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

    /// The backstop: a cell evaluated more than `MAX_EVALS` times is reported, and giving up makes every cell of the request the top, the order of the cells
    /// notwithstanding.
    #[test]
    fn a_cell_evaluated_too_often_makes_every_cell_of_its_request_the_top() {
        let mut run = Run::default();
        let first = run.cell_for((DeclId::new(0, 0), Key::open()));
        let second = run.cell_for((DeclId::new(0, 1), Key::open()));
        for _ in 0..MAX_EVALS {
            assert_eq!(run.settle(first, Value::data()), (false, false));
        }
        assert_eq!(run.settle(first, Value::data()), (false, true));
        run.give_up();
        assert!(run.cells.iter().all(|cell| cell.value.is_top()));
        assert!(run.work.is_empty());
        // and a cell that was never evaluated is the top too
        assert!(run.cells[second].value.is_top());
    }

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

    /// BV-D2, run: with every value unknown the evaluator gives, for every def of every fixture and scenario, what layer 1's item walk gives, item by item: the
    /// sites, their targets, the keys (all open), what is counted.
    #[test]
    fn an_evaluator_that_knows_nothing_is_layer_one() {
        let compared: usize = crate::equal::fixture_books()
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
