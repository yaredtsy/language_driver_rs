//! The flat solver, for a language that uses its engine's own call resolver: memoize one summary per
//! `(function, key)`, follow edges breadth-first, stop at limits, build the [`Graph`].
//!
//! A language plugs in through [`Lang`]. It does not recurse into callees and does not check limits: `walk` reads
//! **one** body, and the solver does the rest. Python does not use this: its context-sensitive solver is its own.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::graph::{
    Basis, Context, ContextId, Cut, DefInfo, Edge, EdgeTo, External, Graph, Reason, Root, Site,
    Stats, Status,
};

/// What one call site can reach.
#[non_exhaustive]
pub enum Target<L: Lang + ?Sized> {
    Call(L::Func, L::Key),
    Construct {
        class: L::Func,
        init: Option<(L::Func, L::Key)>,
    },
    External(External),
    /// The call has no target the engine can give; the reason is the key of the counter in `Stats.language`.
    Unresolved(Reason),
}

pub struct SiteCalls<L: Lang + ?Sized> {
    pub site: Site,
    pub targets: Vec<Target<L>>,
}

impl<L: Lang + ?Sized> SiteCalls<L> {
    pub fn new(site: Site, targets: Vec<Target<L>>) -> Self {
        SiteCalls { site, targets }
    }
}

/// The call sites of one body under one key.
#[non_exhaustive]
pub struct Summary<L: Lang + ?Sized> {
    /// In source order.
    pub sites: Vec<SiteCalls<L>>,
    /// `false`: the walk was cut (cancelled, deadline). Such a summary is never reused.
    pub complete: bool,
}

impl<L: Lang + ?Sized> Summary<L> {
    pub fn complete(sites: Vec<SiteCalls<L>>) -> Self {
        Summary {
            sites,
            complete: true,
        }
    }

    /// A prefix of the real sites: the walk stopped early.
    pub fn cut(sites: Vec<SiteCalls<L>>) -> Self {
        Summary {
            sites,
            complete: false,
        }
    }
}

pub trait Lang {
    /// What this language's resolved edges rest on. An answer from the engine is `Basis::Type`. Required, not defaulted: a language
    /// says it, the core does not assume it.
    const BASIS: Basis;
    /// The engine's handle for a function or class. Only valid during one request.
    type Func: Copy + Eq + Hash;
    /// What a context holds besides the function. `()` for a default resolver.
    type Key: Clone + Eq + Hash;

    /// What the function or class is. Called once per definition per `graph` call.
    fn describe(&self, f: Self::Func) -> DefInfo;
    /// The 16 bytes of `ContextId::digest`. Zeros for `()`.
    fn digest(&self, key: &Self::Key) -> [u8; 16];
    /// Walk ONE body under ONE key. No recursion into callees: the solver does that.
    fn walk(&mut self, f: Self::Func, key: &Self::Key, cancel: &Cancel) -> Summary<Self>;
}

/// A request to stop, and an optional deadline. Cheap to clone; every clone sees the same flag.
#[derive(Debug, Clone, Default)]
pub struct Cancel {
    flag: Arc<AtomicBool>,
    deadline: Option<Instant>,
}

impl Cancel {
    /// Never fires on its own; [`cancel`](Self::cancel) can still fire it.
    pub fn new() -> Self {
        Cancel::default()
    }

    pub fn with_deadline(mut self, at: Instant) -> Self {
        self.deadline = Some(at);
        self
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }

    /// Why to stop now, if to stop. A cancel request wins over the deadline.
    pub fn reason(&self) -> Option<Cut> {
        if self.flag.load(Ordering::Relaxed) {
            Some(Cut::Cancelled)
        } else if self.deadline.is_some_and(|d| Instant::now() >= d) {
            Some(Cut::Deadline)
        } else {
            None
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.reason().is_some()
    }
}

/// Cost limits. They cut, the cut is marked on the context and counted in `Stats`, and a cut result is never cached.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    /// Bodies walked in one `graph` call. A context past it is listed but not walked.
    pub max_contexts: u32,
    /// Steps from an entry. A context at this distance is listed but not walked.
    pub max_depth: Option<u32>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_contexts: 100_000,
            max_depth: Some(1_000),
        }
    }
}

impl Limits {
    pub fn new(max_contexts: u32, max_depth: Option<u32>) -> Self {
        Limits {
            max_contexts,
            max_depth,
        }
    }
}

pub struct Solver<L: Lang> {
    lang: L,
    memo: HashMap<(L::Func, L::Key), Summary<L>>,
    limits: Limits,
}

type Node<L> = (<L as Lang>::Func, <L as Lang>::Key);

impl<L: Lang> Solver<L> {
    pub fn new(lang: L, limits: Limits) -> Self {
        Solver {
            lang,
            memo: HashMap::new(),
            limits,
        }
    }

    pub fn lang(&self) -> &L {
        &self.lang
    }

    pub fn lang_mut(&mut self) -> &mut L {
        &mut self.lang
    }

    /// Forget every summary (a file changed, and the language cannot say which ones).
    pub fn clear(&mut self) {
        self.memo.clear();
    }

    /// One body, memoized: what "expand one node" costs. A summary that was cut is walked again.
    pub fn summary(&mut self, f: L::Func, key: &L::Key, cancel: &Cancel) -> &Summary<L> {
        let node = (f, key.clone());
        self.ensure(&node, cancel);
        &self.memo[&node]
    }

    /// `true` when the summary was already known.
    fn ensure(&mut self, node: &Node<L>, cancel: &Cancel) -> bool {
        if self.memo.get(node).is_some_and(|s| s.complete) {
            return true;
        }
        let summary = self.lang.walk(node.0, &node.1, cancel);
        self.memo.insert(node.clone(), summary);
        false
    }

    /// Everything reachable from `entries`, until a limit. Breadth-first, so which contexts a limit cuts does not
    /// depend on the order the engine lists things in; the result is sorted by [`Graph::new`].
    pub fn graph(&mut self, entries: &[(L::Func, L::Key)], cancel: &Cancel) -> Graph {
        let mut run = Run::<L> {
            seen: HashSet::new(),
            queue: VecDeque::new(),
            infos: HashMap::new(),
        };
        let mut defs = Vec::new();
        let mut contexts = Vec::new();
        let mut edges = Vec::new();
        let mut stats = Stats::default();

        let entry_ids: Vec<ContextId> = entries.iter().map(|n| self.id_of(&mut run, n)).collect();
        for node in entries {
            run.discover(node, 0);
        }

        while let Some((node, depth)) = run.queue.pop_front() {
            let from = self.id_of(&mut run, &node);
            defs.push(info_of(&self.lang, &mut run.infos, node.0).clone());
            let mut context = Context::new(from.clone());
            stats.contexts += 1;

            let stop = if stats.walks >= u64::from(self.limits.max_contexts) {
                Some(Cut::MaxContexts)
            } else if self.limits.max_depth.is_some_and(|d| depth >= d) {
                Some(Cut::MaxDepth)
            } else {
                cancel.reason()
            };
            if let Some(why) = stop {
                context.status = Status::Incomplete(why);
                stats.cut(why);
                contexts.push(context);
                continue;
            }

            let hit = self.ensure(&node, cancel);
            stats.walks += u64::from(!hit);
            stats.reuse_hits += u64::from(hit);

            // `memo` and `lang` are separate fields, so the summary can stay borrowed while we call `describe`.
            let summary = &self.memo[&node];
            if !summary.complete {
                let why = cancel.reason().unwrap_or(Cut::Cancelled);
                context.status = Status::Incomplete(why);
                stats.cut(why);
            }
            for calls in &summary.sites {
                for target in &calls.targets {
                    let to = match target {
                        Target::Call(f, k) => {
                            let callee = (*f, k.clone());
                            run.discover(&callee, depth + 1);
                            EdgeTo::Context(id_of(&self.lang, &mut run.infos, &callee), L::BASIS)
                        }
                        Target::Construct { class, init } => {
                            // A class has no body to walk, so it gets a row in `defs` and no context.
                            let info = info_of(&self.lang, &mut run.infos, *class).clone();
                            let class = info.id.clone();
                            defs.push(info);
                            let init = init.as_ref().map(|(f, k)| {
                                let init = (*f, k.clone());
                                run.discover(&init, depth + 1);
                                id_of(&self.lang, &mut run.infos, &init)
                            });
                            EdgeTo::Construct {
                                class,
                                init,
                                basis: L::BASIS,
                            }
                        }
                        Target::External(e) => EdgeTo::External(e.clone(), L::BASIS),
                        Target::Unresolved(why) => {
                            stats.unresolved += 1;
                            EdgeTo::Unresolved(why.clone())
                        }
                    };
                    edges.push(Edge {
                        from: from.clone(),
                        site: calls.site,
                        to,
                    });
                }
            }
            contexts.push(context);
        }
        Graph::new(
            entry_ids.into_iter().map(Root::Context).collect(),
            defs,
            contexts,
            edges,
            stats,
        )
    }

    fn id_of(&self, run: &mut Run<L>, node: &Node<L>) -> ContextId {
        id_of(&self.lang, &mut run.infos, node)
    }
}

/// State of one `graph` call.
struct Run<L: Lang> {
    seen: HashSet<Node<L>>,
    queue: VecDeque<(Node<L>, u32)>,
    /// `describe` can be expensive in an engine; ask once per function per call.
    infos: HashMap<L::Func, DefInfo>,
}

impl<L: Lang> Run<L> {
    /// Queue a context the first time it is seen, with the depth it was first seen at.
    fn discover(&mut self, node: &Node<L>, depth: u32) {
        if self.seen.insert(node.clone()) {
            self.queue.push_back((node.clone(), depth));
        }
    }
}

fn info_of<'a, L: Lang>(
    lang: &L,
    infos: &'a mut HashMap<L::Func, DefInfo>,
    f: L::Func,
) -> &'a DefInfo {
    infos.entry(f).or_insert_with(|| lang.describe(f))
}

fn id_of<L: Lang>(lang: &L, infos: &mut HashMap<L::Func, DefInfo>, node: &Node<L>) -> ContextId {
    ContextId {
        func: info_of(lang, infos, node.0).id.clone(),
        digest: lang.digest(&node.1),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::graph::StableDef;
    use crate::outline::DefKind;
    use crate::text::Span;

    /// A language whose "program" is a table: function name → the names it calls, one site each, in order.
    /// A name that is not a key is external; `"?"` is unresolved.
    pub struct Toy {
        pub program: HashMap<&'static str, Vec<&'static str>>,
        pub walks: Vec<&'static str>,
    }

    impl Toy {
        pub fn new(program: &[(&'static str, &[&'static str])]) -> Self {
            Toy {
                program: program.iter().map(|(k, v)| (*k, v.to_vec())).collect(),
                walks: Vec::new(),
            }
        }
    }

    pub fn toy_info(f: &'static str) -> DefInfo {
        let start = f.bytes().map(u32::from).sum::<u32>();
        DefInfo::new(
            StableDef::Marked(f.to_owned()),
            f,
            f,
            0,
            DefKind::Function,
            "toy.src",
            Span::new(start, start + 1),
        )
    }

    impl Lang for Toy {
        const BASIS: Basis = Basis::Type;
        type Func = &'static str;
        type Key = ();

        fn describe(&self, f: &'static str) -> DefInfo {
            toy_info(f)
        }

        fn digest(&self, _: &()) -> [u8; 16] {
            [0; 16]
        }

        fn walk(&mut self, f: &'static str, _: &(), _: &Cancel) -> Summary<Self> {
            self.walks.push(f);
            let sites = self
                .program
                .get(f)
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(i, callee)| {
                    let target = match *callee {
                        "?" => Target::Unresolved(Reason::new("toy")),
                        c if self.program.contains_key(c) => Target::Call(c, ()),
                        c => Target::External(External {
                            package: "std".into(),
                            name: c.into(),
                        }),
                    };
                    SiteCalls::new(Site::at(i as u32), vec![target])
                })
                .collect();
            Summary::complete(sites)
        }
    }

    pub(crate) fn no_dangling(g: &Graph) {
        for e in &g.edges {
            assert!(g.context(&e.from).is_some());
            match &e.to {
                EdgeTo::Context(c, _) => assert!(g.context(c).is_some(), "{c:?}"),
                EdgeTo::Construct { class, .. } => {
                    assert!(g.def(class).is_some(), "no row for class {class:?}")
                }
                _ => {}
            }
        }
        for c in &g.contexts {
            assert!(g.def(&c.id.func).is_some(), "no row for {:?}", c.id.func);
        }
    }

    fn graph_of(program: &[(&'static str, &[&'static str])], entry: &'static str) -> Graph {
        let mut solver = Solver::new(Toy::new(program), Limits::default());
        solver.graph(&[(entry, ())], &Cancel::new())
    }

    /// The names of the functions the contexts are of, in context order.
    fn names(g: &Graph) -> Vec<&str> {
        g.contexts
            .iter()
            .map(|c| g.def(&c.id.func).unwrap().qname.as_str())
            .collect()
    }

    fn calls(g: &Graph, from: &str) -> Vec<String> {
        let id = ContextId::of(StableDef::Marked(from.to_owned()));
        g.edges_from(&id)
            .iter()
            .map(|e| match &e.to {
                EdgeTo::Context(c, _) => match &c.func {
                    StableDef::Marked(n) => n.clone(),
                    _ => unreachable!(),
                },
                EdgeTo::External(x, _) => format!("ext:{}", x.name),
                EdgeTo::Unresolved(_) => "?".to_owned(),
                _ => "other".to_owned(),
            })
            .collect()
    }

    #[test]
    fn straight_line() {
        let g = graph_of(&[("a", &["b"]), ("b", &["c"]), ("c", &[])], "a");
        assert_eq!(names(&g), ["a", "b", "c"]);
        assert_eq!(calls(&g, "a"), ["b"]);
        assert_eq!(calls(&g, "b"), ["c"]);
        assert_eq!((g.stats.walks, g.stats.contexts), (3, 3));
        assert!(g.contexts.iter().all(|c| c.status == Status::Finished));
    }

    #[test]
    fn diamond_walks_the_shared_callee_once() {
        let g = graph_of(
            &[("a", &["b", "c"]), ("b", &["d"]), ("c", &["d"]), ("d", &[])],
            "a",
        );
        assert_eq!(names(&g), ["a", "b", "c", "d"]);
        assert_eq!(g.stats.walks, 4);
        assert_eq!(calls(&g, "b"), ["d"]);
        assert_eq!(calls(&g, "c"), ["d"]);
    }

    #[test]
    fn direct_and_mutual_recursion_terminate() {
        let g = graph_of(&[("a", &["a"])], "a");
        assert_eq!(
            (names(&g), calls(&g, "a")),
            (vec!["a"], vec!["a".to_owned()])
        );
        let g = graph_of(&[("a", &["b"]), ("b", &["a"])], "a");
        assert_eq!(names(&g), ["a", "b"]);
        assert_eq!(
            (calls(&g, "a"), calls(&g, "b")),
            (vec!["b".to_owned()], vec!["a".to_owned()])
        );
        assert_eq!(g.stats.walks, 2);
    }

    #[test]
    fn external_and_unresolved_are_edges_not_contexts() {
        let g = graph_of(&[("a", &["print", "?", "?"])], "a");
        assert_eq!(names(&g), ["a"]);
        assert_eq!(calls(&g, "a"), ["ext:print", "?", "?"]);
        assert_eq!(g.stats.unresolved, 2);
    }

    #[test]
    fn edges_carry_the_language_basis_and_unresolved_carries_its_reason() {
        let g = graph_of(&[("a", &["b", "print", "?"]), ("b", &[])], "a");
        let kinds: Vec<_> = g
            .edges
            .iter()
            .map(|e| {
                (
                    e.to.basis(),
                    matches!(&e.to, EdgeTo::Unresolved(r) if r.as_str() == "toy"),
                )
            })
            .collect();
        assert_eq!(
            kinds,
            [
                (Some(Basis::Type), false),
                (Some(Basis::Type), false),
                (None, true)
            ]
        );
    }

    #[test]
    fn two_sites_to_one_callee_are_two_edges() {
        let g = graph_of(&[("a", &["b", "b"]), ("b", &[])], "a");
        assert_eq!(calls(&g, "a"), ["b", "b"]);
    }

    #[test]
    fn max_contexts_lists_but_does_not_walk() {
        let program: &[(&str, &[&str])] =
            &[("a", &["b"]), ("b", &["c"]), ("c", &["d"]), ("d", &[])];
        let mut solver = Solver::new(Toy::new(program), Limits::new(2, None));
        let g = solver.graph(&[("a", ())], &Cancel::new());
        assert_eq!(
            names(&g),
            ["a", "b", "c"],
            "c is known (b calls it) but not walked"
        );
        let status: Vec<_> = g.contexts.iter().map(|c| c.status).collect();
        assert_eq!(
            status,
            [
                Status::Finished,
                Status::Finished,
                Status::Incomplete(Cut::MaxContexts)
            ]
        );
        assert_eq!(g.stats.cuts.get(&Cut::MaxContexts), Some(&1));
        assert!(g.edges_from(&g.contexts[2].id).is_empty());
        assert_eq!(solver.lang().walks, ["a", "b"]);
    }

    #[test]
    fn max_depth_cuts_by_distance_from_the_entry() {
        let program: &[(&str, &[&str])] = &[
            ("a", &["b", "c"]),
            ("b", &["d"]),
            ("c", &[]),
            ("d", &["e"]),
            ("e", &[]),
        ];
        let mut solver = Solver::new(Toy::new(program), Limits::new(100, Some(2)));
        let g = solver.graph(&[("a", ())], &Cancel::new());
        assert_eq!(names(&g), ["a", "b", "c", "d"]);
        let d = g
            .context(&ContextId::of(StableDef::Marked("d".into())))
            .unwrap();
        assert_eq!(d.status, Status::Incomplete(Cut::MaxDepth));
        assert_eq!(g.stats.cuts.get(&Cut::MaxDepth), Some(&1));
    }

    #[test]
    fn cancel_before_the_start_walks_nothing_and_says_so() {
        let mut solver = Solver::new(Toy::new(&[("a", &["b"]), ("b", &[])]), Limits::default());
        let cancel = Cancel::new();
        cancel.cancel();
        let g = solver.graph(&[("a", ())], &cancel);
        assert_eq!(names(&g), ["a"]);
        assert_eq!(g.contexts[0].status, Status::Incomplete(Cut::Cancelled));
        assert!(solver.lang().walks.is_empty());
    }

    #[test]
    fn a_passed_deadline_cuts_with_its_own_reason() {
        let mut solver = Solver::new(Toy::new(&[("a", &[])]), Limits::default());
        let cancel = Cancel::new().with_deadline(Instant::now());
        let g = solver.graph(&[("a", ())], &cancel);
        assert_eq!(g.contexts[0].status, Status::Incomplete(Cut::Deadline));
    }

    #[test]
    fn memo_is_reused_across_graph_calls_and_cut_results_are_not() {
        let mut solver = Solver::new(Toy::new(&[("a", &["b"]), ("b", &[])]), Limits::default());
        solver.graph(&[("a", ())], &Cancel::new());
        let g = solver.graph(&[("a", ())], &Cancel::new());
        assert_eq!(
            solver.lang().walks,
            ["a", "b"],
            "the second call walked nothing"
        );
        assert_eq!((g.stats.walks, g.stats.reuse_hits), (0, 2));

        // `summary` of a cut walk is replaced the next time it is asked for
        struct Flaky(u32);
        impl Lang for Flaky {
            const BASIS: Basis = Basis::Type;
            type Func = u8;
            type Key = ();
            fn describe(&self, _: u8) -> DefInfo {
                toy_info("f")
            }
            fn digest(&self, _: &()) -> [u8; 16] {
                [0; 16]
            }
            fn walk(&mut self, _: u8, _: &(), _: &Cancel) -> Summary<Self> {
                self.0 += 1;
                Summary {
                    sites: vec![],
                    complete: self.0 > 1,
                }
            }
        }
        let mut solver = Solver::new(Flaky(0), Limits::default());
        assert!(!solver.summary(0, &(), &Cancel::new()).complete);
        assert!(solver.summary(0, &(), &Cancel::new()).complete);
        assert!(solver.summary(0, &(), &Cancel::new()).complete);
        assert_eq!(solver.lang().0, 2);
    }

    #[test]
    fn graph_marks_a_context_whose_walk_was_cut() {
        struct Cuts;
        impl Lang for Cuts {
            const BASIS: Basis = Basis::Type;
            type Func = u8;
            type Key = ();
            fn describe(&self, _: u8) -> DefInfo {
                toy_info("f")
            }
            fn digest(&self, _: &()) -> [u8; 16] {
                [0; 16]
            }
            fn walk(&mut self, _: u8, _: &(), cancel: &Cancel) -> Summary<Self> {
                cancel.cancel();
                Summary::cut(vec![])
            }
        }
        let g = Solver::new(Cuts, Limits::default()).graph(&[(0, ())], &Cancel::new());
        assert_eq!(g.contexts[0].status, Status::Incomplete(Cut::Cancelled));
        assert_eq!(g.stats.cuts.get(&Cut::Cancelled), Some(&1));
    }

    /// A fake context-sensitive language: `f` called with key `k` calls `f` with `k + 1` until 3.
    /// Proves the memo is keyed by `(Func, Key)`, and that the digest tells the contexts apart.
    struct Counting;
    impl Lang for Counting {
        const BASIS: Basis = Basis::Type;
        type Func = &'static str;
        type Key = u8;
        fn describe(&self, f: &'static str) -> DefInfo {
            toy_info(f)
        }
        fn digest(&self, key: &u8) -> [u8; 16] {
            [*key; 16]
        }
        fn walk(&mut self, f: &'static str, key: &u8, _: &Cancel) -> Summary<Self> {
            let targets = if *key < 3 {
                vec![Target::Call(f, key + 1)]
            } else {
                vec![]
            };
            Summary::complete(vec![SiteCalls::new(Site::at(0), targets)])
        }
    }

    #[test]
    fn one_function_many_contexts_when_the_key_is_not_unit() {
        let g = Solver::new(Counting, Limits::default()).graph(&[("f", 0)], &Cancel::new());
        assert_eq!(g.contexts.len(), 4);
        assert_eq!(
            g.contexts
                .iter()
                .map(|c| c.id.digest[0])
                .collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert!(
            g.contexts
                .iter()
                .all(|c| c.id.func == StableDef::Marked("f".into()))
        );
        assert_eq!(g.edges.len(), 3);
    }

    #[test]
    fn a_function_with_two_contexts_has_one_def_row() {
        let g = Solver::new(Counting, Limits::default()).graph(&[("f", 0)], &Cancel::new());
        assert_eq!(g.contexts.len(), 4);
        assert_eq!(g.defs.len(), 1);
        assert_eq!(g.defs[0].id, StableDef::Marked("f".into()));
        no_dangling(&g);
    }

    #[test]
    fn construct_edge_carries_the_class_and_queues_the_init() {
        struct Ctor;
        impl Lang for Ctor {
            const BASIS: Basis = Basis::Type;
            type Func = &'static str;
            type Key = ();
            fn describe(&self, f: &'static str) -> DefInfo {
                let mut info = toy_info(f);
                if f == "Foo" {
                    info.kind = DefKind::Class;
                }
                info
            }
            fn digest(&self, _: &()) -> [u8; 16] {
                [0; 16]
            }
            fn walk(&mut self, f: &'static str, _: &(), _: &Cancel) -> Summary<Self> {
                let sites = match f {
                    "main" => vec![SiteCalls::new(
                        Site::at(0),
                        vec![Target::Construct {
                            class: "Foo",
                            init: Some(("Foo.init", ())),
                        }],
                    )],
                    _ => vec![],
                };
                Summary::complete(sites)
            }
        }
        let g = Solver::new(Ctor, Limits::default()).graph(&[("main", ())], &Cancel::new());
        // `main` and `Foo.init` are contexts; the class `Foo` is only a row in `defs`
        assert_eq!(g.contexts.len(), 2);
        assert_eq!(g.stats.contexts, 2);
        assert!(
            g.context(&ContextId::of(StableDef::Marked("Foo".into())))
                .is_none(),
            "a class has no body, so no context"
        );
        let foo = g.def(&StableDef::Marked("Foo".into())).unwrap();
        assert_eq!(foo.kind, DefKind::Class);
        assert_eq!(g.defs.len(), 3, "main, Foo.init, Foo");
        no_dangling(&g);
        let EdgeTo::Construct { class, init, .. } = &g.edges[0].to else {
            panic!("{:?}", g.edges[0])
        };
        assert_eq!(class, &StableDef::Marked("Foo".into()));
        assert_eq!(
            init.as_ref().unwrap().func,
            StableDef::Marked("Foo.init".into())
        );
    }

    #[test]
    fn same_input_same_graph_whatever_the_hash_order() {
        let program: &[(&str, &[&str])] = &[
            ("main", &["a", "b", "c", "d"]),
            ("a", &["e"]),
            ("b", &["e", "f"]),
            ("c", &["f"]),
            ("d", &["a", "main"]),
            ("e", &[]),
            ("f", &["e"]),
        ];
        let first = graph_of(program, "main");
        for _ in 0..20 {
            assert_eq!(graph_of(program, "main"), first);
        }
    }

    #[test]
    fn several_entries_keep_the_requests_order() {
        let mut solver = Solver::new(Toy::new(&[("a", &[]), ("b", &[])]), Limits::default());
        let g = solver.graph(&[("b", ()), ("a", ())], &Cancel::new());
        let order: Vec<_> = g.entries.iter().map(|e| e.func().clone()).collect();
        assert_eq!(
            order,
            [StableDef::Marked("b".into()), StableDef::Marked("a".into())]
        );
        assert_eq!(names(&g), ["a", "b"]);
    }
}
