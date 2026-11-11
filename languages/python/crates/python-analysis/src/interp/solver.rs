// src/interp/solver.rs
use std::sync::Arc;
use std::time::Instant;

use rustc_hash::FxHashMap;

use ruff_index::{Idx, IndexVec};

use crate::interp::defer::{DeferredOp, no_values};
use crate::interp::key::{Context, ContextId, Fp, FpId, Interner};
use crate::interp::summary::{Cut, FunctionResult, Gaps, Status, same_svals};
use crate::interp::walk::{Walk, contains_yield};
use crate::interp::{Options, Stats};
use crate::sem::{Definition, Sem};

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) enum State {
    #[default]
    New,
    Running {
        depth: usize,
    }, // its walk is in progress, at this stack depth
    Parked {
        head: ContextId,
    }, // walked, waiting for its group's fixpoint
    Resolving, // its group is in the fixpoint: `current` is the latest round
    Ready,     // walked: its summary can be reused
    Cut,       // walked, but a cost limit fired: never reused
}

pub(crate) struct Slot<'db> {
    pub state: State,
    pub result: Option<Arc<FunctionResult<'db>>>,
    /// The latest summary, during the fixpoint.
    pub current: Option<Arc<FunctionResult<'db>>>,
    /// Smallest stack depth of a running context this walk read. `usize::MAX`: none.
    pub lowest: usize,
    /// The walk, with its deferred operations: kept while Parked or Resolving.
    pub walk: Option<Box<Walk<'db>>>,
    /// A resolving group this walk reached: it joins that group.
    pub joins: Option<ContextId>,
    /// A cost limit this walk (or something it read) hit.
    pub cut: Option<Cut>,
}

impl Default for Slot<'_> {
    fn default() -> Self {
        Slot {
            state: State::New,
            result: None,
            current: None,
            lowest: usize::MAX,
            walk: None,
            joins: None,
            cut: None,
        }
    }
}

impl<'db> Slot<'db> {
    fn current(&self) -> Arc<FunctionResult<'db>> {
        self.current
            .clone()
            .expect("a group member has a current summary")
    }

    /// Store the round's summary. True when what a caller reads changed.
    fn set_current(&mut self, next: FunctionResult<'db>) -> bool {
        let changed = self
            .current
            .as_ref()
            .is_none_or(|c| !same_svals(&c.returns, &next.returns));
        self.current = Some(Arc::new(next));
        changed
    }
}

/// Union-find over contexts: the head of the group a parked context belongs to.
#[derive(Default)]
pub(crate) struct Heads {
    parent: Vec<ContextId>,
}

impl Heads {
    fn grow(&mut self, ctx: ContextId) {
        while self.parent.len() <= ctx.index() {
            self.parent.push(ContextId::new(self.parent.len()));
        }
    }
    pub(crate) fn find(&mut self, ctx: ContextId) -> ContextId {
        self.grow(ctx);
        let mut root = ctx;
        while self.parent[root.index()] != root {
            root = self.parent[root.index()];
        }
        let mut c = ctx;
        while self.parent[c.index()] != root {
            c = std::mem::replace(&mut self.parent[c.index()], root);
        }
        root
    }
    /// `ctx`'s whole group goes under `head`'s.
    fn union_into(&mut self, ctx: ContextId, head: ContextId) {
        let (a, b) = (self.find(ctx), self.find(head));
        self.parent[a.index()] = b;
    }
    fn reset(&mut self, ctx: ContextId) {
        self.grow(ctx);
        self.parent[ctx.index()] = ctx;
    }
}

/// A group's fixpoint in progress. Fixpoints nest like calls.
#[derive(Clone, Copy)]
pub(crate) struct Fixpoint {
    pub head: ContextId,
    /// The stack depth the operations are replayed at.
    pub base: usize,
    /// Smallest depth below `base` an operation read. `usize::MAX`: none.
    pub lowest: usize,
    /// A resolving group an operation reached.
    pub joins: Option<ContextId>,
    /// A cost limit an operation hit.
    pub cut: Option<Cut>,
}

pub(crate) enum Resolved {
    Done,
    Merged(usize), // part of a bigger cycle, whose head is at this depth
    #[allow(dead_code)] // the group it joined is found by other means; the id is never read
    Joined(ContextId), // its replay reached an outer group's fixpoint
}

pub(crate) enum Got<'db> {
    Ready(Arc<FunctionResult<'db>>),
    Pending, // the context is running: a recursive call, its value found later
}

/// What outlives one walk: the request's options, interners, slots and counters.
pub(crate) struct Solver<'db> {
    pub sem: Sem<'db>,
    pub opts: Options,
    pub fps: Interner<Fp<'db>, FpId>,
    pub contexts: Interner<Context<'db>, ContextId>,
    pub slots: IndexVec<ContextId, Slot<'db>>, // summary mode: one per context
    pub stack: Vec<ContextId>,                 // summary mode: the walks in progress
    pub heads: Heads,
    pub fixpoint: Option<Fixpoint>,
    pub per_func: FxHashMap<Definition<'db>, u32>, // contexts walked per function, this request
    pub started: Instant,
    pub parked: Vec<ContextId>, // parked contexts, in the order their walks ended
    pub path: Vec<String>, // inline mode only: qualified names being walked, its recursion guard
    pub stats: Stats,
}

impl<'db> Solver<'db> {
    pub(crate) fn new(sem: Sem<'db>, opts: Options) -> Self {
        Solver {
            sem,
            opts,
            fps: Interner::default(),
            contexts: Interner::default(),
            slots: IndexVec::new(),
            stack: Vec::new(),
            heads: Heads::default(),
            fixpoint: None,
            per_func: FxHashMap::default(),
            started: Instant::now(),
            parked: Vec::new(),
            path: Vec::new(),
            stats: Stats::default(),
        }
    }

    /// The summary of `ctx`: walked now, or handed out from its slot.
    pub(crate) fn get(&mut self, ctx: ContextId) -> Got<'db> {
        while self.slots.len() < self.contexts.len() {
            self.slots.push(Slot::default());
        }
        let depth = self.stack.len();
        match self.slots[ctx].state {
            State::Ready if self.opts.limits.reuse => {
                self.stats.reuse_hits += 1;
                return Got::Ready(self.slots[ctx].result.clone().unwrap());
            }
            State::Running { depth: d } => {
                self.reader_depends_on(d);
                return Got::Pending;
            }
            State::Parked { .. } | State::Resolving => return self.read_group_member(ctx),
            _ => {} // New, Cut, or a Ready slot in no-reuse mode
        }
        if let Some(cut) = self.cost_limit(ctx, depth) {
            self.reader_read_cut(cut);
            return Got::Ready(Arc::new(FunctionResult::cut(cut))); // not stored
        }

        self.heads.reset(ctx);
        self.slots[ctx].state = State::Running { depth };
        self.slots[ctx].lowest = usize::MAX;
        self.slots[ctx].joins = None;
        self.slots[ctx].cut = None;
        self.stack.push(ctx);
        let w = walk_context(self, ctx);
        self.stack.pop();

        let (lowest, joins) = (self.slots[ctx].lowest, self.slots[ctx].joins);
        if lowest < depth {
            // In a cycle whose head is further down the stack: park.
            let head = self.heads.find(self.stack[lowest]);
            self.park(ctx, w, head);
            self.reader_depends_on(lowest); // the caller is in the cycle too
            if let Some(j) = joins {
                self.reader_joins(j); // and reaches the resolving group too
            }
            return Got::Pending;
        }
        if let Some(head) = joins {
            self.park(ctx, w, head); // first reached during a fixpoint: join it
            return self.read_group_member(ctx);
        }
        if lowest == depth {
            match self.resolve_group(ctx, w) {
                Resolved::Done => {}
                Resolved::Merged(d) => {
                    self.reader_depends_on(d);
                    return Got::Pending;
                }
                Resolved::Joined(_) => return self.read_group_member(ctx),
            }
        } else {
            self.finish_alone(ctx, w);
        }
        let result = self.slots[ctx].result.clone().unwrap();
        self.pass_cut_up(&result);
        Got::Ready(result)
    }

    /// The first cost limit that fires for a context about to be walked.
    fn cost_limit(&mut self, ctx: ContextId, depth: usize) -> Option<Cut> {
        let l = &self.opts.limits;
        if depth >= l.max_stack {
            return Some(Cut::Stack);
        }
        if self.stats.walks >= l.max_contexts {
            return Some(Cut::Contexts);
        }
        let n = self.per_func.entry(self.contexts[ctx].func).or_default();
        if *n >= l.contexts_per_func {
            return Some(Cut::ContextsPerFunc);
        }
        if l.deadline.is_some_and(|d| self.started.elapsed() > d) {
            return Some(Cut::Deadline);
        }
        *n += 1;
        None
    }

    /// The reader (the top of the stack, or the fixpoint replaying an
    /// operation) read a cut result.
    fn reader_read_cut(&mut self, cut: Cut) {
        self.stats.cuts.count(cut);
        self.reader_is_cut(cut);
    }

    /// After `get` produced a result: a cut in it makes the reader cut.
    fn pass_cut_up(&mut self, res: &FunctionResult<'db>) {
        if let Some(cut) = res.status.cut {
            self.reader_is_cut(cut);
        }
    }

    fn reader_is_cut(&mut self, cut: Cut) {
        if let Some(f) = self
            .fixpoint
            .as_mut()
            .filter(|f| f.base == self.stack.len())
        {
            f.cut.get_or_insert(cut);
        } else if let Some(&reader) = self.stack.last() {
            self.slots[reader].cut.get_or_insert(cut);
        }
    }

    /// A walk that waited on nothing: its summary is final.
    fn finish_alone(&mut self, ctx: ContextId, w: Walk<'db>) {
        debug_assert!(
            w.deferred.is_empty(),
            "{ctx:?} finishes alone with deferred operations"
        );
        let mut result = w.finish_round(self, &no_values(&w));
        let cut = self.slots[ctx].cut;
        result.status.cut = cut;
        self.slots[ctx].state = if cut.is_some() {
            State::Cut
        } else {
            State::Ready
        };
        self.slots[ctx].result = Some(Arc::new(result));
    }

    /// A read of a parked or resolving context.
    fn read_group_member(&mut self, ctx: ContextId) -> Got<'db> {
        let head = self.heads.find(ctx);
        let state = self.slots[head].state;
        match state {
            State::Running { depth } => {
                self.reader_depends_on(depth); // still being walked
                Got::Pending
            }
            _ if self.reader_is_fixpoint_of(head) => Got::Ready(self.slots[ctx].current()),
            _ => {
                self.reader_joins(head); // a new walk, or another group's fixpoint
                Got::Pending
            }
        }
    }

    /// The reader is the replay of `head`'s own fixpoint, not a walk on top of
    /// it and not a fixpoint nested inside it.
    fn reader_is_fixpoint_of(&mut self, head: ContextId) -> bool {
        let Some(f) = self.fixpoint else { return false };
        self.stack.len() == f.base && self.heads.find(f.head) == head
    }

    /// Back to round 0: the group's fixpoint (re)starts this member.
    fn resolving_from_round0(&mut self, m: ContextId) {
        let mut w = self.slots[m].walk.take().expect("a member keeps its walk");
        self.slots[m].current = Some(Arc::new(w.finish_round(self, &no_values(&w))));
        w.resolved = no_values(&w);
        self.slots[m].walk = Some(w);
        self.slots[m].state = State::Resolving;
    }

    /// Keep a walked context under `head` until its group resolves.
    fn park(&mut self, ctx: ContextId, w: Walk<'db>, head: ContextId) {
        if self.slots[ctx].current.is_none() {
            // A joining context is read at once: it needs its round-0 summary now.
            self.slots[ctx].current = Some(Arc::new(w.finish_round(self, &no_values(&w))));
        }
        self.heads.union_into(ctx, head); // whatever is parked under ctx follows it
        self.slots[ctx].walk = Some(Box::new(w));
        self.slots[ctx].state = State::Parked { head };
        self.parked.push(ctx);
    }

    /// Members whose fixpoint stopped go under another head, walks and summaries kept.
    fn park_all(&mut self, members: &[ContextId], head: ContextId) {
        let head = self.heads.find(head);
        for &m in members {
            self.heads.union_into(m, head);
            self.slots[m].state = State::Parked { head };
            self.parked.push(m);
        }
    }

    /// Every context parked under `head`, in walk-end order.
    fn take_parked(&mut self, head: ContextId) -> Vec<ContextId> {
        let all = std::mem::take(&mut self.parked);
        let (mine, rest): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|&c| self.heads.find(c) == head);
        self.parked = rest;
        mine
    }

    /// The head of a cycle: one fixpoint over every context parked under it.
    fn resolve_group(&mut self, head: ContextId, head_walk: Walk<'db>) -> Resolved {
        self.park(head, head_walk, head);
        let mut members = self.take_parked(head);
        for &m in &members {
            self.resolving_from_round0(m);
        }
        // Fixpoints nest: this one may run inside an operation of another. Keep it.
        let outer = self.fixpoint.replace(Fixpoint {
            head,
            base: self.stack.len(),
            lowest: usize::MAX,
            joins: None,
            cut: None,
        });

        let mut round = 0;
        let resolved = 'rounds: loop {
            round += 1;
            let capped = round > self.opts.limits.max_rounds;
            let mut changed = false;
            for &m in &members {
                let next = self.replay_member(m, capped);
                let f = self.fixpoint.unwrap();
                if f.lowest != usize::MAX {
                    // reached a context running further down: part of a bigger cycle
                    let under = self.stack[f.lowest];
                    self.keep_cut(&members, f.cut);
                    self.park_all(&members, under);
                    break 'rounds Resolved::Merged(f.lowest);
                }
                if let Some(g) = f.joins {
                    // reached an outer group in its fixpoint
                    self.keep_cut(&members, f.cut);
                    self.park_all(&members, g);
                    break 'rounds Resolved::Joined(g);
                }
                changed |= self.slots[m].set_current(next);
            }
            let joined = self.take_parked(head);
            changed |= !joined.is_empty();
            for &j in &joined {
                self.resolving_from_round0(j);
            }
            members.extend(joined);
            if !changed || capped {
                if capped {
                    self.stats.rounds_capped += 1;
                }
                let cut = self.fixpoint.unwrap().cut;
                self.settle(
                    &members,
                    if capped { Gaps::CYCLE } else { Gaps::default() },
                    cut,
                );
                break Resolved::Done;
            }
        };
        self.fixpoint = outer;
        resolved
    }

    /// One member's deferred operations, then its summary for the round.
    fn replay_member(&mut self, m: ContextId, capped: bool) -> FunctionResult<'db> {
        let mut w = self.slots[m].walk.take().expect("a member keeps its walk");
        w.resolved = w.replay(self, capped);
        let next = w.finish_round(self, &w.resolved);
        self.slots[m].walk = Some(w);
        next
    }

    /// The group is final: every member's last summary is its result.
    /// A cut an operation hit stays with the members while they wait.
    fn keep_cut(&mut self, members: &[ContextId], cut: Option<Cut>) {
        if let Some(cut) = cut {
            for &m in members {
                self.slots[m].cut.get_or_insert(cut);
            }
        }
    }

    fn settle(&mut self, members: &[ContextId], gaps: Gaps, fixpoint_cut: Option<Cut>) {
        let group = self.heads.find(members[0]);
        for &m in members {
            for d in &self.slots[m].walk.as_ref().unwrap().deferred {
                if let DeferredOp::Apply { ctx } = d.op {
                    debug_assert_eq!(
                        self.heads.find(ctx),
                        group,
                        "{m:?} waited on {ctx:?}, outside its group"
                    );
                }
            }
        }
        // If any member read a cut, every member is cut.
        let cut = fixpoint_cut.or_else(|| members.iter().find_map(|&m| self.slots[m].cut));
        for &m in members {
            let slot = &mut self.slots[m];
            let mut result = Arc::unwrap_or_clone(slot.current.take().unwrap());
            result.status = Status { gaps, cut };
            slot.result = Some(Arc::new(result));
            slot.walk = None;
            slot.cut = cut;
            slot.state = if cut.is_some() {
                State::Cut
            } else {
                State::Ready
            };
        }
    }

    /// The context on top of the stack, or the fixpoint when an operation is
    /// being replayed, read a running context at depth `d`.
    fn reader_depends_on(&mut self, d: usize) {
        if let Some(f) = self
            .fixpoint
            .as_mut()
            .filter(|f| f.base == self.stack.len())
        {
            f.lowest = f.lowest.min(d);
        } else if let Some(&reader) = self.stack.last() {
            let slot = &mut self.slots[reader];
            slot.lowest = slot.lowest.min(d);
        }
    }

    /// The reader reached a resolving group, and joins it.
    fn reader_joins(&mut self, head: ContextId) {
        if let Some(f) = self
            .fixpoint
            .as_mut()
            .filter(|f| f.base == self.stack.len())
        {
            f.joins.get_or_insert(head);
        } else if let Some(&reader) = self.stack.last() {
            self.slots[reader].joins.get_or_insert(head);
        }
    }

    /// Its summary is missing what later rounds would have added.
    /// Is this function part of the view? Project code, and with `only_marked` one
    /// that has a docstring `ID:`.
    pub(crate) fn shown(&self, def: Definition<'db>) -> bool {
        self.sem.is_project(def) && (!self.opts.only_marked || self.sem.docstring_id(def).is_some())
    }
}

/// Walks the body once. The summary is the solver's to finish: it may need rounds.
pub(crate) fn walk_context<'db>(s: &mut Solver<'db>, ctx: ContextId) -> Walk<'db> {
    let func = s.contexts[ctx].func;
    let parsed = s.sem.parsed(func);
    let node = s
        .sem
        .function_node(func, &parsed)
        .expect("contexts are only made for defs");
    s.stats.walks += 1;
    let mut w = Walk::new(s, func);
    w.generator = contains_yield(&node.body);
    let env = w.materialize(s, ctx, node);
    if let Some(end) = w.walk_stmts(s, &node.body, env) {
        let none = w.none_value(s);
        w.frame.returns.extend(none); // fell off the end
        w.frame.exits.push(end);
    }
    // The run as it ended: a returned closure reads its free values from here.
    let exits = std::mem::take(&mut w.frame.exits);
    if let Some(end) = crate::interp::env::Env::join(exits.into_iter().map(Some).collect()) {
        w.activations[w.frame.act] = end;
    }
    w
}
