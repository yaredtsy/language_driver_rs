//! `Sem::flow`: reading a def's body into a `Flow` (`crate::flow`), the one place the table is made.
//!
//! The table is read **once** per def and kept for as long as the program is loaded, so that a later question ("what is this callee, what is each argument") is
//! asked of plain data and not of the checker. Nothing here lets an engine pointer out: `Flow` holds numbers and owned values, and a def is named by a `DefId`,
//! a number of this program (`Flows`), which `Sem::func` turns back into a `Func` only for the program that made it.

use std::sync::atomic::{AtomicU32, Ordering};

use indexmap::IndexSet;
use tsrs_ast::{Symbol, SymbolFlags};
use tsrs_checker::{Type, TypeFlags};

use super::collect::{CallNode, SiteKind, unwrap_expression};
use super::handed::HandOvers;
use super::view::{
    Binary, Conditional, Parameter, arguments_of, expression_of, initializer_of, parameters_of,
};
use super::*;
use crate::flow::{
    AnonFlow, Arg, DefId, Expr, Flow, FlowKind, HandedFlow, Param, SiteFlow, Var, VarId,
};

/// The tables of one program, and the numbers they are written in.
pub(super) struct Flows {
    /// This program's number: from `PROGRAMS`, taken when the `Sem` is made.
    program: u32,
    /// The files that have a number: a file's number is its place here, given on first use. (The prompt of this step said `P<Node>`; a `P<SourceFile>` is what
    /// a `File` holds, so the file comes back from its number without a downcast of the engine's.)
    files: RefCell<IndexSet<P<SourceFile>>>,
    memo: RefCell<HashMap<DefId, Arc<Flow>>>,
    /// What each def needs from the functions around it (`needs::needs_of_component`), kept with the tables it is made from: a fact of the program, not of a
    /// request, so a request that reaches a def another already asked about does not compute its component again.
    needs: RefCell<HashMap<DefId, Arc<[VarId]>>>,
}

/// The next program's number. It never reaches an output: it only tells two programs' numbers apart.
static PROGRAMS: AtomicU32 = AtomicU32::new(0);

impl Flows {
    pub(super) fn new() -> Flows {
        Flows {
            program: PROGRAMS.fetch_add(1, Ordering::Relaxed),
            files: RefCell::new(IndexSet::new()),
            memo: RefCell::new(HashMap::new()),
            needs: RefCell::new(HashMap::new()),
        }
    }
}

impl Sem {
    /// `func` as a number. The file gets its number the first time one of its defs is asked for.
    pub(crate) fn def_id(&self, func: Func<'_>) -> DefId {
        let (file, _) = self.flows.files.borrow_mut().insert_full(func.file.node);
        let index = func
            .outline_index()
            .map(|index| u32::try_from(index).expect("an outline has fewer than 2^32 defs"));
        DefId::new(
            self.flows.program,
            u32::try_from(file).expect("a program has fewer than 2^32 files"),
            index,
        )
    }

    /// What `def` needs, when a request of this program has already worked it out (`remember_needs`).
    pub(crate) fn needs_memo(&self, def: DefId) -> Option<Arc<[VarId]>> {
        self.flows.needs.borrow().get(&def).map(Arc::clone)
    }

    /// Keep what a component of defs needs (`needs::needs_of_component`), for every request of this program: plain data in, plain data out.
    pub(crate) fn remember_needs(&self, entries: Vec<(DefId, Box<[VarId]>)>) {
        let mut memo = self.flows.needs.borrow_mut();
        for (def, set) in entries {
            memo.insert(def, Arc::from(set));
        }
    }

    /// For tests: whether `flow` has read `def`'s table in this program.
    #[cfg(test)]
    pub(crate) fn has_flow(&self, def: DefId) -> bool {
        self.flows.memo.borrow().contains_key(&def)
    }

    /// The def a number stands for. `None` for a number of another program, or a file number this one never gave.
    pub(crate) fn func(&self, def: DefId) -> Option<Func<'_>> {
        if def.program() != self.flows.program {
            return None;
        }
        let node = *self
            .flows
            .files
            .borrow()
            .get_index(usize::try_from(def.file()).ok()?)?;
        let file = File {
            node,
            _sem: PhantomData,
            _one_thread: PhantomData,
        };
        Some(match def.index() {
            Some(index) => Func::def(file, usize::try_from(index).ok()?),
            None => Func::module(file),
        })
    }

    /// The name of the variable `id` and which row of that name it is among the rows of its def (by place): what a captured variable is written as in a digest.
    /// `None` for a number of another program, or a place that holds no name.
    pub(crate) fn var_name(&self, id: VarId, style: MarkerStyle<'_>) -> Option<(String, u16)> {
        let func = self.func(id.def)?;
        let flow = self.flow(id.def, style)?;
        let text = func.file.node.text();
        let name_at = |at: u32| -> String {
            text.get(at as usize..)
                .unwrap_or("")
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                .collect()
        };
        let name = name_at(id.at);
        if name.is_empty() {
            return None;
        }
        let nth = flow
            .vars
            .iter()
            .filter(|var| var.at < id.at && name_at(var.at) == name)
            .count();
        Some((name, u16::try_from(nth).unwrap_or(u16::MAX)))
    }

    /// The table of `def`'s body. `None` when `func` is `None` or the def has no body to read (as `body`). Read once and kept.
    pub(crate) fn flow(&self, def: DefId, style: MarkerStyle<'_>) -> Option<Arc<Flow>> {
        if let Some(found) = self.flows.memo.borrow().get(&def) {
            return Some(Arc::clone(found));
        }
        let func = self.func(def)?;
        let body = self.body(func, style)?;
        // the defs the body hands over, as D22 finds them: it takes the checker itself and gives it back, so it is asked before the table takes one
        // inside a count of its own, so what it meets is the table's and never leaks into the request's
        let (hand_overs, unhandled_handed) = counted(|| self.handed_over(func, style));
        let defs = self.file_defs(func.file, style)?;
        // a number can only come from `def_id`, which makes an index of the outline; this keeps a made-up one from reaching `node_of`'s indexing
        if func
            .outline_index()
            .is_some_and(|index| index >= defs.nodes.len())
        {
            return None;
        }
        let node = self.node_of(func, style)?;
        let _scope = self.region.enter();
        let mut checker = self.checker();
        let (made, unhandled) = counted(|| {
            let mut reader = Reader {
                sem: self,
                checker: &mut checker,
                file: func.file,
                defs: &defs,
                style,
                def: node,
                params: parameters_of(node)
                    .iter()
                    .copied()
                    .filter(|&parameter| !ast::is_this_parameter(parameter))
                    .collect(),
                anon_at: body
                    .anons
                    .iter()
                    .enumerate()
                    .map(|(index, &span)| (span, index as u32))
                    .collect(),
            };
            reader.read(&body, &hand_overs)
        });
        let made = Arc::new(Flow {
            unhandled: unhandled + unhandled_handed,
            ..made
        });
        self.flows.memo.borrow_mut().insert(def, Arc::clone(&made));
        Some(made)
    }
}

/// A declaration that has a row in `Flow::vars`, before its type and writes are looked at.
struct Declaration {
    /// The name, an `Identifier`.
    name: P<Node>,
    param: Option<u32>,
    open: bool,
    initializer: Option<P<Node>>,
}

/// What one write of a variable does to its row.
enum Write {
    /// `x = value` (and `||=`, `??=`, `&&=`): `value` is one more thing assigned.
    Assigns(Expr),
    /// A write the row models as "anything".
    Open,
    /// A write none of the rows covers: open, and counted.
    Unmodelled,
}

/// What one reading of a body carries: the checker (one handle for the whole body), the file's defs, and where each anonymous function is.
struct Reader<'a, 'p> {
    sem: &'p Sem,
    checker: &'a mut Checker,
    file: File<'p>,
    defs: &'a FileDefs,
    style: MarkerStyle<'a>,
    /// The def's own declaration.
    def: P<Node>,
    /// The parameter nodes of that declaration, in order, without a `this` parameter: `Flow::params` is one row for each, and `Expr::Param(i)` is a place in it.
    params: Vec<P<Node>>,
    /// The place of each anonymous function of the body, by its span.
    anon_at: HashMap<Span, u32>,
}

impl Reader<'_, '_> {
    fn read(&mut self, body: &Body, hand_overs: &HandOvers<'_>) -> Flow {
        let (mut params, symbols): (Vec<Param>, Vec<Option<P<Symbol>>>) = self
            .params
            .clone()
            .into_iter()
            .map(|parameter| self.param(parameter))
            .unzip();
        // one walk of the whole def for every write: the parameters' `assigned` and the variables' rows both come from it
        let writes = written_identifiers(self.checker, self.def, self.file.node.text());
        for (row, symbol) in params.iter_mut().zip(&symbols) {
            if row.named && row.capable {
                row.assigned =
                    symbol.is_some_and(|symbol| writes.iter().any(|(at, _)| *at == symbol));
            }
        }
        let vars = self.vars(&params, &writes);
        let params: Box<[Param]> = params.into();
        let anons: Box<[AnonFlow]> = body.anons.iter().map(|&span| AnonFlow { span }).collect();
        let sites = body
            .sites
            .iter()
            .map(|call| self.site(call, &anons))
            .collect();
        let handed = self.handed(body, hand_overs);
        Flow {
            params,
            sites,
            anons,
            handed,
            // said by the caller, which counts the whole reading
            unhandled: 0,
            vars: vars.0,
            unmodelled: vars.1,
            around: self.around(),
        }
    }

    /// The def of the outline whose node holds this def's node, the nearest one: the first node above that is in the file's `def_of`. `None` when the first
    /// thing found is the `SourceFile` (a def at the top of its file), and for the file's own code, which has nothing above it.
    fn around(&self) -> Option<DefId> {
        let mut up = self.def.parent();
        while let Some(node) = up {
            if let Some(&index) = self.defs.def_of.get(&node) {
                return Some(self.sem.def_id(Func::def(self.file, index)));
            }
            up = node.parent();
        }
        None
    }

    /// The declarations of the def that have a row (`07-tc3` §2), found by one walk of its own node that does not enter a def of the outline (as `Collector`
    /// does not), in source order.
    fn declarations(&mut self) -> Vec<Declaration> {
        let mut found = Vec::new();
        // the file's own code, in a file that is not a module: its top-level variables are globals
        let in_script = self.def.kind() == Kind::SourceFile
            && !ast::is_external_or_common_js_module(self.file.node);
        let mut pending = vec![self.def];
        while let Some(node) = pending.pop() {
            if node != self.def && self.defs.def_of.contains_key(&node) {
                continue;
            }
            match node.kind() {
                Kind::Parameter => {
                    let own = self.params.iter().position(|&param| param == node);
                    let parent_ok = own.is_some()
                        || node.parent().is_some_and(|parent| {
                            matches!(
                                parent.kind(),
                                Kind::ArrowFunction | Kind::FunctionExpression
                            )
                        });
                    if !parent_ok {
                        // a parameter of a signature or a function type: no variable, and nothing in it is read as code
                        continue;
                    }
                    if let Some(parameter) = Parameter::cast(node)
                        && !ast::is_this_parameter(node)
                    {
                        let name = parameter.name();
                        if name.kind() == Kind::Identifier {
                            found.push(Declaration {
                                name,
                                param: own.map(|index| index as u32),
                                open: own.is_none(),
                                initializer: None,
                            });
                        }
                    }
                }
                Kind::BindingElement => {
                    if let Some(name) = node.name()
                        && name.kind() == Kind::Identifier
                    {
                        found.push(Declaration {
                            name,
                            param: None,
                            open: true,
                            initializer: None,
                        });
                    }
                }
                Kind::VariableDeclaration => {
                    if let Some(name) = node.name()
                        && name.kind() == Kind::Identifier
                    {
                        let parent = node.parent();
                        let in_loop =
                            parent
                                .and_then(|list| list.parent())
                                .is_some_and(|statement| {
                                    matches!(
                                        statement.kind(),
                                        Kind::ForOfStatement | Kind::ForInStatement
                                    )
                                });
                        let in_catch =
                            parent.is_some_and(|parent| parent.kind() == Kind::CatchClause);
                        let ambient =
                            ast::get_combined_node_flags(node).intersects(NodeFlags::Ambient);
                        let open = in_loop || in_catch || ambient;
                        found.push(Declaration {
                            name,
                            param: None,
                            // in a script, any other script may write the variable (`07-tc3` §5): open, with the initializer kept
                            open: open || in_script,
                            initializer: if open { None } else { initializer_of(node) },
                        });
                    }
                }
                _ => {}
            }
            node.for_each_child(&mut |child| {
                pending.push(child);
                false
            });
        }
        found
    }

    /// The `vars` of the table and the count of writes no row covers (`07-tc3` §2). `params` are the rows already read, for `capable` of an own parameter;
    /// `writes` is every write of the def, sorted by place.
    fn vars(&mut self, params: &[Param], writes: &[(P<Symbol>, P<Node>)]) -> (Box<[Var]>, u64) {
        let text = self.file.node.text();
        let mut unmodelled = 0;
        let mut vars: Vec<Var> = Vec::new();
        for declaration in self.declarations() {
            let at = own_span(text, declaration.name).start;
            let symbol = self
                .checker
                .get_symbol_at_location_exported(declaration.name);
            let capable = match declaration.param {
                Some(index) => params[index as usize].capable,
                None => symbol.is_some_and(|symbol| {
                    let ty = self.checker.get_type_of_symbol(symbol);
                    !holds_only_data(self.checker, ty, 4)
                }),
            };
            let mut open = declaration.open;
            let mut assigned = Vec::new();
            if capable {
                if let Some(initializer) = declaration.initializer {
                    assigned.push(self.expr_of(initializer));
                }
                for (_, write) in writes
                    .iter()
                    .filter(|(written, _)| Some(*written) == symbol)
                {
                    match self.write(*write) {
                        Write::Assigns(value) => assigned.push(value),
                        Write::Open => open = true,
                        Write::Unmodelled => {
                            open = true;
                            unmodelled += 1;
                        }
                    }
                }
            }
            vars.push(Var {
                at,
                param: declaration.param,
                capable,
                assigned: assigned.into(),
                open: capable && open,
            });
        }
        vars.sort_by_key(|var| var.at);
        (vars.into(), unmodelled)
    }

    /// What one write of a variable is, by the node that holds the written name (`07-tc3` §2's table).
    fn write(&mut self, write: P<Node>) -> Write {
        let Some(parent) = write.parent() else {
            return Write::Unmodelled;
        };
        match parent.kind() {
            Kind::BinaryExpression => match Binary::cast(parent) {
                Some(binary) if binary.left() == write => match binary.operator() {
                    Kind::EqualsToken
                    | Kind::BarBarEqualsToken
                    | Kind::QuestionQuestionEqualsToken
                    | Kind::AmpersandAmpersandEqualsToken => {
                        // `[x = b] = arr`: the binary is itself an element of a destructuring assignment, so `x` is the element or the default
                        if ast::is_assignment_target(parent) {
                            Write::Open
                        } else {
                            Write::Assigns(self.expr_of(binary.right()))
                        }
                    }
                    _ => Write::Open,
                },
                _ => Write::Unmodelled,
            },
            Kind::PrefixUnaryExpression
            | Kind::PostfixUnaryExpression
            | Kind::ForOfStatement
            | Kind::ForInStatement
            | Kind::ArrayLiteralExpression
            | Kind::ShorthandPropertyAssignment
            | Kind::PropertyAssignment
            | Kind::SpreadElement
            | Kind::SpreadAssignment => Write::Open,
            _ => Write::Unmodelled,
        }
    }

    /// One row of `Flow::params`: what `06-tc2` §2 needs to know of the parameter `node`, and the parameter's symbol (`None` when it is not named). The row's
    /// `assigned` is left `false`: `read` says it for all the rows with one walk of the def.
    fn param(&mut self, node: P<Node>) -> (Param, Option<P<Symbol>>) {
        let Some(parameter) = Parameter::cast(node) else {
            return (
                Param {
                    rest: false,
                    named: false,
                    default: None,
                    assigned: false,
                    capable: false,
                },
                None,
            );
        };
        let name = parameter.name();
        let named = name.kind() == Kind::Identifier;
        let default = parameter
            .initializer()
            .map(|initializer| self.expr_of(initializer));
        let symbol = if named {
            self.checker.get_symbol_at_location_exported(name)
        } else {
            None
        };
        let capable = symbol.is_some_and(|symbol| {
            let ty = self.checker.get_type_of_symbol(symbol);
            !holds_only_data(self.checker, ty, 4)
        });
        (
            Param {
                rest: parameter.is_rest(),
                named,
                default,
                assigned: false,
                capable,
            },
            symbol,
        )
    }

    /// The hand-overs of the body: the defs `handed_over` found (it is the one place that knows how a name in a hand-over position stands for a def), then the
    /// names of `Body::refs` it has no def for, through `expr_of`. Sorted by place, ties in the order made.
    fn handed(&mut self, body: &Body, hand_overs: &HandOvers<'_>) -> Box<[HandedFlow]> {
        let mut handed: Vec<HandedFlow> = hand_overs
            .handed
            .iter()
            .map(|found| HandedFlow {
                at: found.at,
                value: Expr::Def(self.sem.def_id(found.func)),
                written: found.written,
            })
            .collect();
        let with_a_def: HashSet<Span> = hand_overs
            .handed
            .iter()
            .filter_map(|found| found.at)
            .collect();
        for &(span, node) in &body.refs {
            if !with_a_def.contains(&span) {
                handed.push(HandedFlow {
                    at: Some(span),
                    value: self.expr_of(node),
                    written: false,
                });
            }
        }
        handed.sort_by_key(|entry| entry.at.map(|span| (span.start, span.end)));
        handed.into()
    }

    fn site(&mut self, call: &CallNode, anons: &[AnonFlow]) -> SiteFlow {
        let kind = match call.kind {
            SiteKind::Call if call.call.kind() == Kind::NewExpression => FlowKind::New,
            SiteKind::Call => FlowKind::Call,
            SiteKind::Jsx => FlowKind::Jsx,
            SiteKind::Tagged => FlowKind::Tagged,
            SiteKind::Decorator => FlowKind::Decorator,
            SiteKind::Accessor(_) => FlowKind::Accessor,
        };
        let callee = self.expr_of(call.expr);
        let args = match kind {
            FlowKind::Call | FlowKind::New => Some(
                arguments_of(call.call)
                    .iter()
                    .map(|&argument| self.argument(argument))
                    .collect(),
            ),
            // the attributes are read from TC4 on
            FlowKind::Jsx => Some(
                vec![Arg {
                    value: Expr::Unknown,
                    spread: false,
                }]
                .into(),
            ),
            FlowKind::Tagged | FlowKind::Decorator | FlowKind::Accessor => None,
        };
        // the last one that holds the site is the innermost: an anonymous function is listed before the ones written inside it
        let anon = anons
            .iter()
            .rposition(|anon| anon.span.start <= call.span.start && call.span.end <= anon.span.end)
            .map(|index| index as u32);
        SiteFlow {
            span: call.span,
            kind,
            callee,
            args,
            anon,
        }
    }

    fn argument(&mut self, argument: P<Node>) -> Arg {
        if argument.kind() == Kind::SpreadElement {
            return Arg {
                value: expression_of(argument).map_or(Expr::Unknown, |inner| self.expr_of(inner)),
                spread: true,
            };
        }
        Arg {
            value: self.expr_of(argument),
            spread: false,
        }
    }

    /// The place in this def's parameters of the one `name` stands for: its symbol's value declaration is one of `self.params`. The symbol is not followed
    /// through an alias: a parameter is never an import.
    fn param_index(&mut self, name: P<Node>) -> Option<u32> {
        let symbol = self.checker.get_symbol_at_location_exported(name)?;
        let declaration = symbol.value_declaration()?;
        let index = self.params.iter().position(|&param| param == declaration)?;
        Some(u32::try_from(index).expect("a def has fewer than 2^32 parameters"))
    }

    /// The variable `name` stands for: its symbol's value declaration is a variable, a parameter or a binding element of this file with a plain name, and the
    /// def that declares it is this def or one around it. An import (`SymbolFlags::Alias`, `export { x } from` too) is followed to what it names
    /// (`imported_var`): the variable of another project file that is at the top of it.
    fn var_of(&mut self, name: P<Node>) -> Option<VarId> {
        let symbol = self.checker.get_symbol_at_location_exported(name)?;
        if symbol.flags().intersects(SymbolFlags::Alias) {
            let aliased = self.checker.get_aliased_symbol(symbol);
            return self.imported_var(aliased);
        }
        let declaration = symbol.value_declaration()?;
        if !matches!(
            declaration.kind(),
            Kind::VariableDeclaration | Kind::Parameter | Kind::BindingElement
        ) {
            return None;
        }
        let declared = declaration
            .name()
            .filter(|name| name.kind() == Kind::Identifier)?;
        if ast::get_source_file_of_node(declaration)? != self.file.node {
            return None;
        }
        // the first def of the outline (or the file) above the declaration
        let mut up = declaration.parent();
        let holder = loop {
            let node = up?;
            if node.kind() == Kind::SourceFile || self.defs.def_of.contains_key(&node) {
                break node;
            }
            up = node.parent();
        };
        // it must be this def, or hold it
        let mut around = Some(self.def);
        loop {
            let node = around?;
            if node == holder {
                break;
            }
            around = node.parent();
        }
        let func = match self.defs.def_of.get(&holder) {
            Some(&index) => Func::def(self.file, index),
            None => Func::module(self.file),
        };
        Some(VarId {
            def: self.sem.def_id(func),
            at: own_span(self.file.node.text(), declared).start,
        })
    }

    /// The variable an import names (`TC3 step 4`): `symbol` is what the alias resolves to. Accepted only when its value declaration is a `VariableDeclaration`
    /// with a plain name, at the **top** of a project file (the first def of that file's outline, or the file, above it is the file), so the row is in that file's
    /// own table (`Func::module`). A namespace import, a function, a class, a variable in a function or a namespace, and a variable of a file that is not project
    /// code (a `.d.ts`, `node_modules`) are not.
    fn imported_var(&mut self, symbol: P<Symbol>) -> Option<VarId> {
        let declaration = symbol.value_declaration()?;
        if declaration.kind() != Kind::VariableDeclaration {
            return None;
        }
        let declared = declaration
            .name()
            .filter(|name| name.kind() == Kind::Identifier)?;
        let file = File {
            node: ast::get_source_file_of_node(declaration)?,
            _sem: PhantomData,
            _one_thread: PhantomData,
        };
        if !self.sem.is_project_code(file) {
            return None;
        }
        let defs = self.sem.file_defs(file, self.style)?;
        let mut up = declaration.parent();
        loop {
            let node = up?;
            if node.kind() == Kind::SourceFile {
                break;
            }
            if defs.def_of.contains_key(&node) {
                return None;
            }
            up = node.parent();
        }
        Some(VarId {
            def: self.sem.def_id(Func::module(file)),
            at: own_span(file.node.text(), declared).start,
        })
    }

    /// What the expression is, as far as the code alone says (`crate::flow::Expr`).
    fn expr_of(&mut self, node: P<Node>) -> Expr {
        let node = unwrap_expression(node);
        // a def of this file's outline: itself, but a class is TC5's
        if let Some(&index) = self.defs.def_of.get(&node) {
            if self.defs.outline.defs[index].kind == DefKind::Class {
                return Expr::Unknown;
            }
            return Expr::Def(self.sem.def_id(Func::def(self.file, index)));
        }
        match node.kind() {
            Kind::ArrowFunction | Kind::FunctionExpression => {
                let span = own_span(self.file.node.text(), node);
                self.anon_at
                    .get(&span)
                    .map_or(Expr::Unknown, |&index| Expr::Anon(index))
            }
            Kind::Identifier | Kind::PropertyAccessExpression => {
                if node.kind() == Kind::Identifier
                    && let Some(index) = self.param_index(node)
                {
                    return Expr::Param(index);
                }
                let (functions, classes) =
                    self.sem
                        .functions_named(&mut *self.checker, node, self.style);
                if classes > 0 || functions.is_empty() {
                    return if node.kind() == Kind::Identifier {
                        self.var_of(node).map_or(Expr::Unknown, Expr::Var)
                    } else {
                        Expr::Unknown
                    };
                }
                // one symbol can stand for one function twice (a script's function merged with a library global of the same name, the DOM's `open`): `one_of` keeps it once
                one_of(
                    functions
                        .into_iter()
                        .map(|function| Expr::Def(self.sem.def_id(function))),
                )
            }
            Kind::ConditionalExpression => match Conditional::expect(node) {
                Some(conditional) => {
                    let when_true = self.expr_of(conditional.when_true());
                    let when_false = self.expr_of(conditional.when_false());
                    one_of([when_true, when_false])
                }
                None => Expr::Unknown,
            },
            Kind::BinaryExpression => {
                let Some(binary) = Binary::expect(node) else {
                    return Expr::Unknown;
                };
                match binary.operator() {
                    Kind::BarBarToken
                    | Kind::QuestionQuestionToken
                    | Kind::AmpersandAmpersandToken => {
                        let left = self.expr_of(binary.left());
                        let right = self.expr_of(binary.right());
                        one_of([left, right])
                    }
                    operator if is_data_operator(operator) => Expr::Data,
                    _ => Expr::Unknown,
                }
            }
            Kind::StringLiteral
            | Kind::NumericLiteral
            | Kind::BigIntLiteral
            | Kind::RegularExpressionLiteral
            | Kind::NoSubstitutionTemplateLiteral
            | Kind::TemplateExpression
            | Kind::TrueKeyword
            | Kind::FalseKeyword
            | Kind::NullKeyword
            | Kind::PrefixUnaryExpression
            | Kind::PostfixUnaryExpression
            | Kind::TypeOfExpression
            | Kind::VoidExpression
            | Kind::DeleteExpression
            | Kind::JsxElement
            | Kind::JsxSelfClosingElement
            | Kind::JsxFragment => Expr::Data,
            _ => Expr::Unknown,
        }
    }
}

/// Whether every value of `ty` is data (nothing callable, no object): what makes a parameter not capable (`06-tc2` §2). A union whose members all are;
/// a primitive, `void`, `null`, `undefined`, `never`, a literal or template literal type, an enum (`TypeFlags::Primitive | TypeFlags::Never`); an array
/// whose element type is. Anything else is not, and so is anything met `depth` levels down (a recursive alias stops there, capable). A tuple is not
/// looked into: its element types come only through `get_type_arguments`, which downcasts with an assert.
fn holds_only_data(checker: &mut Checker, ty: P<Type>, depth: u8) -> bool {
    if depth == 0 {
        return false;
    }
    if ty.flags().intersects(TypeFlags::Union) {
        return ty
            .types()
            .iter()
            .all(|&member| holds_only_data(checker, member, depth - 1));
    }
    if ty
        .flags()
        .intersects(TypeFlags::Primitive | TypeFlags::Never)
    {
        return true;
    }
    checker.is_array_type(ty)
        && checker
            .get_element_type_of_array_type_exported(ty)
            .is_some_and(|element| holds_only_data(checker, element, depth - 1))
}

/// Every write under `container` (its parameters, its body, and the defs and anonymous functions written in it): each `Identifier` that `ast::is_write_access`
/// says is written (`=`, a compound assignment, `++`, a destructuring or loop target) and has a symbol, with that symbol, sorted by the identifier's place. The
/// write test is syntax only, so it comes first and the checker is asked only about writes. A declaration's own name is never one: `is_write_access` says
/// `Read` for it. One walk per def (`06-tc2` §10, step 4d; TC3 step 1).
///
/// The walk keeps its own stack of nodes still to visit instead of calling itself, so a deep tree cannot overflow the thread's stack.
fn written_identifiers(
    checker: &mut Checker,
    container: P<Node>,
    text: &str,
) -> Vec<(P<Symbol>, P<Node>)> {
    let mut written: Vec<(P<Symbol>, P<Node>)> = Vec::new();
    let mut pending = vec![container];
    while let Some(node) = pending.pop() {
        if node.kind() == Kind::Identifier
            && ast::is_write_access(node)
            && let Some(symbol) = checker.get_symbol_at_location_exported(node)
        {
            written.push((symbol, node));
        }
        node.for_each_child(&mut |child| {
            pending.push(child);
            false
        });
    }
    written.sort_by_key(|(_, node)| own_span(text, *node).start);
    written
}

/// The only maker of `Expr::OneOf`, so that it is never nested, never lists a member twice and always has two or more. A member that is itself a `OneOf`
/// puts its members in its place; a member equal to one before it is dropped (the first is kept, the order is kept); and when one member is left, it is
/// the answer and not a `OneOf` of one.
fn one_of(members: impl IntoIterator<Item = Expr>) -> Expr {
    let mut flat: Vec<Expr> = Vec::new();
    for member in members {
        let parts = match member {
            Expr::OneOf(inner) => Vec::from(inner),
            other => vec![other],
        };
        for part in parts {
            if !flat.contains(&part) {
                flat.push(part);
            }
        }
    }
    if flat.len() == 1 {
        flat.remove(0)
    } else {
        Expr::OneOf(flat.into())
    }
}

/// The operators of a binary expression whose value is data: arithmetic, shifts, bitwise, comparison, equality, `instanceof`, `in`, and the compound
/// assignments of those (`x += 1` is a number).
fn is_data_operator(operator: Kind) -> bool {
    matches!(
        operator,
        Kind::PlusToken
            | Kind::MinusToken
            | Kind::AsteriskToken
            | Kind::SlashToken
            | Kind::PercentToken
            | Kind::AsteriskAsteriskToken
            | Kind::LessThanLessThanToken
            | Kind::GreaterThanGreaterThanToken
            | Kind::GreaterThanGreaterThanGreaterThanToken
            | Kind::AmpersandToken
            | Kind::BarToken
            | Kind::CaretToken
            | Kind::LessThanToken
            | Kind::GreaterThanToken
            | Kind::LessThanEqualsToken
            | Kind::GreaterThanEqualsToken
            | Kind::EqualsEqualsToken
            | Kind::ExclamationEqualsToken
            | Kind::EqualsEqualsEqualsToken
            | Kind::ExclamationEqualsEqualsToken
            | Kind::InstanceOfKeyword
            | Kind::InKeyword
            | Kind::PlusEqualsToken
            | Kind::MinusEqualsToken
            | Kind::AsteriskEqualsToken
            | Kind::SlashEqualsToken
            | Kind::PercentEqualsToken
            | Kind::AsteriskAsteriskEqualsToken
            | Kind::LessThanLessThanEqualsToken
            | Kind::GreaterThanGreaterThanEqualsToken
            | Kind::GreaterThanGreaterThanGreaterThanEqualsToken
            | Kind::AmpersandEqualsToken
            | Kind::BarEqualsToken
            | Kind::CaretEqualsToken
    )
}

#[cfg(test)]
mod tests;
