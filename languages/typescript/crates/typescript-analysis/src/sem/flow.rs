//! `Sem::flow`: reading a def's body into a `Flow` (`crate::flow`), the one place the table is made.
//!
//! The table is read **once** per def and kept for as long as the program is loaded, so that a later question ("what is this callee, what is each argument") is
//! asked of plain data and not of the checker. Nothing here lets an engine pointer out: `Flow` holds numbers and owned values, and a def is named by a `DefId`,
//! a number of this program (`Flows`), which `Sem::func` turns back into a `Func` only for the program that made it.

use std::sync::atomic::{AtomicU32, Ordering};

use indexmap::IndexSet;
use tsrs_ast::modifierflags::ModifierFlags;
use tsrs_ast::{Symbol, SymbolFlags};
use tsrs_checker::{Type, TypeFlags};

use super::collect::{CallNode, SiteKind, unwrap_expression};
use super::handed::HandOvers;
use super::view::{
    Binary, Binding, CallOrNew, Conditional, ElementAccess, NameToken, Parameter, PrefixUnary,
    Wrapper, arguments_of, elements_of, expression_of, initializer_of, parameters_of,
    properties_of, spelled, tag_name_of,
};
use super::*;
use crate::flow::{
    AnonFlow, Arg, DefId, Expr, Flow, FlowKind, HandedFlow, Param, Part, SiteFlow, Uses, Var, VarId,
};

/// A fact of a def kept per bit: the def, and whether the layer that asked knows `Caps::OBJECTS`.
type PerBit<T> = HashMap<(DefId, bool), T>;

/// The tables of one program, and the numbers they are written in.
pub(super) struct Flows {
    /// This program's number: from `PROGRAMS`, taken when the `Sem` is made.
    program: u32,
    /// The files that have a number: a file's number is its place here, given on first use. (The prompt of this step said `P<Node>`; a `P<SourceFile>` is what
    /// a `File` holds, so the file comes back from its number without a downcast of the engine's.)
    files: RefCell<IndexSet<P<SourceFile>>>,
    memo: RefCell<HashMap<DefId, Arc<Flow>>>,
    /// What each def needs from the functions around it (`needs::needs_of_component`), kept with the tables it is made from: a fact of the program, not of a
    /// request, so a request that reaches a def another already asked about does not compute its component again. Kept per bit: whether the layer knows
    /// `Caps::OBJECTS` (the second of the key) changes what a table reads (`needs::each_expr`), so a request with the bit never sees what one without it kept.
    needs: RefCell<PerBit<Arc<[VarId]>>>,
    /// Which parameters of each def can reach a call (`relevance::relevant`), kept the same way.
    relevance: RefCell<PerBit<Arc<[bool]>>>,
    /// Whether each variable's object can be trusted (`trust::trust_of_component`), kept the same way as `relevance`: a fact of the code.
    trust: RefCell<HashMap<VarId, bool>>,
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
            relevance: RefCell::new(HashMap::new()),
            trust: RefCell::new(HashMap::new()),
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

    /// What `def` needs, when a request of this program with the same `objects` has already worked it out (`remember_needs`).
    pub(crate) fn needs_memo(&self, def: DefId, objects: bool) -> Option<Arc<[VarId]>> {
        self.flows
            .needs
            .borrow()
            .get(&(def, objects))
            .map(Arc::clone)
    }

    /// Keep what a component of defs needs (`needs::needs_of_component`), for every request of this program with the same `objects`: plain data in, plain data out.
    pub(crate) fn remember_needs(&self, entries: Vec<(DefId, Box<[VarId]>)>, objects: bool) {
        let mut memo = self.flows.needs.borrow_mut();
        for (def, set) in entries {
            memo.insert((def, objects), Arc::from(set));
        }
    }

    /// Which parameters of `def` can reach a call, when a request of this program with the same `objects` has already worked it out (`remember_relevance`).
    pub(crate) fn relevance_memo(&self, def: DefId, objects: bool) -> Option<Arc<[bool]>> {
        self.flows
            .relevance
            .borrow()
            .get(&(def, objects))
            .map(Arc::clone)
    }

    /// Keep which parameters of `def` can reach a call, for every request of this program with the same `objects`: plain data.
    pub(crate) fn remember_relevance(&self, def: DefId, objects: bool, flags: Arc<[bool]>) {
        self.flows
            .relevance
            .borrow_mut()
            .insert((def, objects), flags);
    }

    /// Whether the object `var` holds can be trusted, when a request of this program has already worked it out (`remember_trust`).
    pub(crate) fn trust_memo(&self, var: VarId) -> Option<bool> {
        self.flows.trust.borrow().get(&var).copied()
    }

    /// Keep which variables' objects can be trusted (`trust::trust_of_component`), for every request of this program: plain data in, plain data out.
    pub(crate) fn remember_trust(&self, entries: Vec<(VarId, bool)>) {
        self.flows.trust.borrow_mut().extend(entries);
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
                exports: None,
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
    initial: Option<Initial>,
}

/// Where a declaration's first value comes from.
enum Initial {
    /// A variable declaration's initializer.
    Node(P<Node>),
    /// A binding element of an object pattern: its value is its path (`Reader::element_value`).
    Element(P<Node>),
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
    /// The symbols the file's module exports, and what each export alias names (`exported_symbols`), asked once and only for a row of the file's own code.
    exports: Option<HashSet<P<Symbol>>>,
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
        // a pattern parameter's uses are the variables of its pattern: the table reads them from the declaration, which `self.params` still holds
        for (row, &node) in params.iter_mut().zip(&self.params.clone()) {
            if !row.named
                && let Some(parameter) = Parameter::cast(node)
            {
                match self.pattern_vars(parameter.name()) {
                    Some(names) => row.uses.names = sorted(names).into(),
                    None => row.uses.escapes = true,
                }
            }
        }
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
            this_uses: self.this_uses(),
        }
    }

    /// What the def does with its `this` (`08-tc4` §10, step 3c). `Some` for a function declaration, a function expression, and a method or accessor of an
    /// object literal: every `this` of its body is placed as a reference is, into one `Found`. A nested def with a `this` of its own is not entered (a function,
    /// a method, an accessor, a constructor, a class); an arrow is, since its `this` is this def's. `None` for the other defs, whose `this` is not a variable.
    fn this_uses(&mut self) -> Option<Uses> {
        let has_own_this = match self.def.kind() {
            Kind::FunctionDeclaration | Kind::FunctionExpression => true,
            Kind::MethodDeclaration | Kind::GetAccessor | Kind::SetAccessor => self
                .def
                .parent()
                .is_some_and(|parent| parent.kind() == Kind::ObjectLiteralExpression),
            _ => false,
        };
        if !has_own_this {
            return None;
        }
        let mut found = Found::default();
        let mut pending = Vec::new();
        self.def.for_each_child(&mut |child| {
            pending.push(child);
            false
        });
        while let Some(node) = pending.pop() {
            match node.kind() {
                Kind::FunctionDeclaration
                | Kind::FunctionExpression
                | Kind::MethodDeclaration
                | Kind::GetAccessor
                | Kind::SetAccessor
                | Kind::Constructor
                | Kind::ClassDeclaration
                | Kind::ClassExpression => continue,
                Kind::ThisKeyword => self.place(node, &mut found),
                _ => {}
            }
            node.for_each_child(&mut |child| {
                pending.push(child);
                false
            });
        }
        Some(Uses {
            escapes: found.escapes,
            passed: sorted(found.passed).into(),
            names: sorted(found.names).into(),
            receiver: found.receiver,
            exported: false,
        })
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
                                initial: None,
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
                            open: false,
                            initial: Some(Initial::Element(node)),
                        });
                    }
                }
                Kind::VariableDeclaration => {
                    if let Some(name) = node.name()
                        && name.kind() == Kind::Identifier
                    {
                        found.push(Declaration {
                            name,
                            param: None,
                            open: self.declaration_is_open(node),
                            // a loop's, a catch's and an ambient declaration's initializer is not its value; a script's top-level one is still its first value
                            initial: if is_open_by_form(node) {
                                None
                            } else {
                                initializer_of(node).map(Initial::Node)
                            },
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

    /// Whether a variable declaration is one the table keeps open: a loop's, a catch's, an ambient one, and in a script the top level's (any other script may
    /// write a variable of the file's top level, `07-tc3` §5; one inside a function written in the file, an arrow or a function expression, is that function's own,
    /// not a global). Both `declarations` and `element_value` ask it.
    fn declaration_is_open(&self, declaration: P<Node>) -> bool {
        let in_script = self.def.kind() == Kind::SourceFile
            && !ast::is_external_or_common_js_module(self.file.node);
        is_open_by_form(declaration) || (in_script && !inside_a_function(declaration, self.def))
    }

    /// The value of a binding element's name, **its own default included** (`08-tc4` §2): its path from the source of its pattern. `None` when the table cannot
    /// say: an array pattern, a source that is not this def's own parameter, a declaration's initializer or an enclosing element, a declaration that is open
    /// (`declaration_is_open`), a computed key.
    fn element_value(&mut self, element: P<Node>) -> Option<Expr> {
        let pattern = element
            .parent()
            .filter(|parent| parent.kind() == Kind::ObjectBindingPattern)?;
        let holder = pattern.parent()?;
        let source = match holder.kind() {
            Kind::Parameter => {
                // a rest parameter holds the list, not one value
                if Parameter::cast(holder)?.is_rest() {
                    return None;
                }
                let index = self.params.iter().position(|&param| param == holder)?;
                Expr::Param(u32::try_from(index).expect("a def has fewer than 2^32 parameters"))
            }
            Kind::VariableDeclaration => {
                if self.declaration_is_open(holder) {
                    return None;
                }
                self.expr_of(initializer_of(holder)?)
            }
            Kind::BindingElement => self.element_value(holder)?,
            _ => return None,
        };
        let value = if Binding::cast(element)?.is_rest() {
            let mut before = Vec::new();
            for &earlier in elements_of(pattern) {
                if earlier == element {
                    break;
                }
                before.push(self.element_key(earlier)?);
            }
            omit(source, before.into())
        } else {
            member(source, self.element_key(element)?)
        };
        Some(match initializer_of(element) {
            Some(default) => {
                let default = self.expr_of(default);
                Expr::Default(Box::new(value), Box::new(default))
            }
            None => value,
        })
    }

    /// The member of its source a binding element reads: its `propertyName` when it has one (`{ a: b }`), else its name, which must be an identifier.
    fn element_key(&self, element: P<Node>) -> Option<Box<str>> {
        let name = match element.property_name() {
            Some(name) => name,
            None => element
                .name()
                .filter(|name| name.kind() == Kind::Identifier)?,
        };
        self.property_name(name)
    }

    /// A property name (`08-tc4` §2): an identifier's text, a string literal's or a no-substitution template's value (unquoted), a numeric literal's text as
    /// written. A computed name and a private name are no name.
    fn property_name(&self, name: P<Node>) -> Option<Box<str>> {
        let text = self.file.node.text();
        match name.kind() {
            Kind::Identifier | Kind::StringLiteral | Kind::NoSubstitutionTemplateLiteral => {
                NameToken::cast(name).map(|token| token.value().into())
            }
            Kind::NumericLiteral => own_span(text, name).slice(text).map(Into::into),
            _ => None,
        }
    }

    /// The `vars` of the table and the count of writes no row covers (`07-tc3` §2). `params` are the rows already read, for `capable` of an own parameter;
    /// `writes` is every write of the def, sorted by place.
    fn vars(&mut self, params: &[Param], writes: &[(P<Symbol>, P<Node>)]) -> (Box<[Var]>, u64) {
        let text = self.file.node.text();
        let mut unmodelled = 0;
        let mut rows: Vec<(Declaration, Option<P<Symbol>>, bool)> = Vec::new();
        for declaration in self.declarations() {
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
            rows.push((declaration, symbol, capable));
        }
        // every reference of a capable variable, from one walk of the whole def; an identifier is looked up only when its name is one of theirs
        let wanted: HashSet<&str> = rows
            .iter()
            .filter(|(_, _, capable)| *capable)
            .filter_map(|(declaration, _, _)| NameToken::cast(declaration.name))
            .map(NameToken::value)
            .collect();
        let references = if wanted.is_empty() {
            HashMap::new()
        } else {
            referenced_identifiers(self.checker, self.def, &wanted)
        };
        let mut vars: Vec<Var> = Vec::new();
        for (declaration, symbol, capable) in rows {
            let at = own_span(text, declaration.name).start;
            let mut open = declaration.open;
            let mut assigned = Vec::new();
            let mut uses = Uses::default();
            if capable {
                match declaration.initial {
                    Some(Initial::Node(initializer)) => assigned.push(self.expr_of(initializer)),
                    Some(Initial::Element(element)) => match self.element_value(element) {
                        Some(value) => assigned.push(value),
                        None => open = true,
                    },
                    None => {}
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
                if let Some(symbol) = symbol {
                    uses = self.uses_of(declaration.name, symbol, &references);
                }
            }
            vars.push(Var {
                at,
                param: declaration.param,
                capable,
                assigned: assigned.into(),
                open: capable && open,
                uses,
            });
        }
        vars.sort_by_key(|var| var.at);
        (vars.into(), unmodelled)
    }

    /// What the body does with the variable `name` declares (`symbol`): each of its references, placed, and whether the file exports it.
    fn uses_of(
        &mut self,
        name: P<Node>,
        symbol: P<Symbol>,
        references: &HashMap<P<Symbol>, Vec<P<Node>>>,
    ) -> Uses {
        let mut found = Found::default();
        for &reference in references.get(&symbol).map_or(&[][..], Vec::as_slice) {
            self.place(reference, &mut found);
        }
        // only a row of the file's own code can be a module's export
        let exported = self.def.kind() == Kind::SourceFile && self.is_exported(name, symbol);
        Uses {
            escapes: found.escapes,
            passed: sorted(found.passed).into(),
            names: sorted(found.names).into(),
            receiver: found.receiver,
            exported,
        }
    }

    /// Whether the file exports the variable `name` declares: its declaration's statement has the `export` modifier (`get_combined_modifier_flags` walks up from
    /// a binding element or a declaration to its statement), or the module's exports hold its symbol, or an alias among them names it (`export { x }`,
    /// `export default x`). The module's own symbol is `get_symbol_of_declaration_exported` of the file; its exports are `get_exports_of_module_exported`, each
    /// alias followed by `get_aliased_symbol`. A script has no module symbol, so no export.
    fn is_exported(&mut self, name: P<Node>, symbol: P<Symbol>) -> bool {
        if name.parent().is_some_and(|declaration| {
            ast::get_combined_modifier_flags(declaration).intersects(ModifierFlags::Export)
        }) {
            return true;
        }
        if self.exports.is_none() {
            let mut found = HashSet::new();
            if let Some(module) = self
                .checker
                .get_symbol_of_declaration_exported(self.file.node.as_node())
            {
                for export in self.checker.get_exports_of_module_exported(module) {
                    found.insert(export);
                    if export.flags().intersects(SymbolFlags::Alias) {
                        found.insert(self.checker.get_aliased_symbol(export));
                    }
                }
            }
            self.exports = Some(found);
        }
        self.exports
            .as_ref()
            .is_some_and(|exports| exports.contains(&symbol))
    }

    /// The variables a binding pattern declares, nested patterns included, in source order; `None` when an element has no name the table can use. Each is
    /// named by its declaring def (`declared_var`), wherever the pattern is in this def's node: a pattern in a nested def is that def's row.
    fn pattern_vars(&mut self, pattern: P<Node>) -> Option<Vec<VarId>> {
        let mut found = Vec::new();
        let mut pending = vec![pattern];
        while let Some(pattern) = pending.pop() {
            for &element in elements_of(pattern) {
                // a hole of an array pattern (`[, a]`)
                if element.kind() != Kind::BindingElement {
                    continue;
                }
                let name = element.name()?;
                match name.kind() {
                    Kind::Identifier => found.push(self.declared_var(name)?),
                    Kind::ObjectBindingPattern | Kind::ArrayBindingPattern => pending.push(name),
                    _ => return None,
                }
            }
        }
        Some(found)
    }

    /// One reference of a variable, put into what the body does with its value (`08-tc4` §5, the numbered rules of step 3a). A reference that is itself written is
    /// no use of the value (the row's `assigned` and `open` say it). Otherwise the reference is climbed past the nodes that only wrap it, and its parent says.
    fn place(&mut self, reference: P<Node>, found: &mut Found) {
        // a shorthand property `{ x }` is the reference, and the identifier it holds is what is written in `({ x } = o)`
        let written = if reference.kind() == Kind::ShorthandPropertyAssignment {
            reference.name()
        } else {
            Some(reference)
        };
        if written.is_some_and(ast::is_write_access) {
            return;
        }
        self.place_used(wrapped(reference), found);
    }

    /// `place` for a node that is already the topmost wrapper over a value of the variable (the reference itself, or the top of a read chain): points 1–10.
    fn place_used(&mut self, used: P<Node>, found: &mut Found) {
        let Some(parent) = used.parent() else {
            found.escapes = true;
            return;
        };
        // 1. the object of an access chain
        if matches!(
            parent.kind(),
            Kind::PropertyAccessExpression | Kind::ElementAccessExpression
        ) && expression_of(parent) == Some(used)
        {
            let mut chain = parent;
            let top = loop {
                let outer = wrapped(chain);
                // `(x.a as any) = f` writes through a wrapper the access's own test does not see past
                if ast::is_write_access(chain)
                    || ast::is_write_access(outer)
                    || outer
                        .parent()
                        .is_some_and(|above| above.kind() == Kind::DeleteExpression)
                {
                    found.escapes = true;
                    return;
                }
                match outer.parent() {
                    Some(next)
                        if matches!(
                            next.kind(),
                            Kind::PropertyAccessExpression | Kind::ElementAccessExpression
                        ) && expression_of(next) == Some(outer) =>
                    {
                        chain = next;
                    }
                    _ => break outer,
                }
            };
            // `new x.C()` runs a constructor: `this` there is the new object, not `x`
            if top.parent().is_some_and(|above| {
                above.kind() == Kind::CallExpression
                    && CallOrNew::cast(above).and_then(CallOrNew::callee) == Some(top)
            }) {
                found.receiver = true;
                return;
            }
            // a member that is data (`x.label`) keeps nothing of `x`; one that can hold a function or an object is a value of `x` that the body may then keep or
            // write through (`const b = x.b; b.c = f`), so it is placed as the reference is
            let ty = self.checker.get_type_at_location(top);
            if holds_only_data(self.checker, ty, 4) {
                return;
            }
            self.place_used(top, found);
            return;
        }
        // 2. the callee of a call or a `new`
        if let Some(call) = CallOrNew::cast(parent)
            && call.callee() == Some(used)
        {
            return;
        }
        // 3. a condition, or a place whose value is only tested
        if is_tested(used) {
            return;
        }
        // 4. a side of `&&`, `||`, `??` whose value is itself only tested
        if let Some(binary) = Binary::cast(parent)
            && matches!(
                binary.operator(),
                Kind::AmpersandAmpersandToken | Kind::BarBarToken | Kind::QuestionQuestionToken
            )
        {
            if !is_tested(wrapped(parent)) {
                found.escapes = true;
            }
            return;
        }
        // 5. a spread copies the members, it does not keep the object
        if matches!(
            parent.kind(),
            Kind::SpreadAssignment | Kind::JsxSpreadAttribute
        ) {
            return;
        }
        // 6. an argument of a call or a `new`
        if let Some(call) = CallOrNew::cast(parent) {
            let index = arguments_of(parent)
                .iter()
                .position(|&argument| argument == used);
            let target = call.callee().map(unwrap_expression);
            match (index, target) {
                (Some(index), Some(target)) => {
                    let index = u32::try_from(index).expect("a call has fewer than 2^32 arguments");
                    self.pass_to(target, index, found);
                }
                _ => found.escapes = true,
            }
            return;
        }
        // 7. the value of a JSX attribute: the component's first parameter
        if parent.kind() == Kind::JsxExpression
            && expression_of(parent) == Some(used)
            && let Some(attribute) = parent
                .parent()
                .filter(|attribute| attribute.kind() == Kind::JsxAttribute)
            && initializer_of(attribute) == Some(parent)
        {
            let tag = attribute
                .parent()
                .and_then(|attributes| attributes.parent())
                .and_then(tag_name_of);
            match tag {
                Some(tag) => self.pass_to(tag, 0, found),
                None => found.escapes = true,
            }
            return;
        }
        // 8. a declaration's initializer: a second name (an object pattern: its variables)
        if parent.kind() == Kind::VariableDeclaration
            && initializer_of(parent) == Some(used)
            && let Some(name) = parent.name()
        {
            match name.kind() {
                Kind::Identifier => match self.declared_var(name) {
                    Some(var) => found.names.push(var),
                    None => found.escapes = true,
                },
                Kind::ObjectBindingPattern => match self.pattern_vars(name) {
                    Some(names) => found.names.extend(names),
                    None => found.escapes = true,
                },
                _ => found.escapes = true,
            }
            return;
        }
        // 9. the right side of `y = x`
        if let Some(binary) = Binary::cast(parent)
            && binary.operator() == Kind::EqualsToken
            && binary.right() == used
        {
            let left = unwrap_expression(binary.left());
            match (left.kind() == Kind::Identifier)
                .then(|| self.declared_var(left))
                .flatten()
            {
                Some(var) => found.names.push(var),
                None => found.escapes = true,
            }
            return;
        }
        // 10. anything else: a return, a store, an array element, a template, a place the rule does not list
        found.escapes = true;
    }

    /// `target` (a callee, or a component's tag name) given the value as argument `index`: `passed` when the engine names project functions and no class there,
    /// else the value escapes (a library, an intrinsic tag, a class, an expression that names nothing).
    fn pass_to(&mut self, target: P<Node>, index: u32, found: &mut Found) {
        if !matches!(
            target.kind(),
            Kind::Identifier | Kind::PropertyAccessExpression
        ) {
            found.escapes = true;
            return;
        }
        let (functions, classes) = self
            .sem
            .functions_named(&mut *self.checker, target, self.style);
        if functions.is_empty() || classes > 0 {
            found.escapes = true;
            return;
        }
        for function in functions {
            found.passed.push((self.sem.def_id(function), index));
        }
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
                    uses: Uses::default(),
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
        let capable = if named {
            symbol.is_some_and(|symbol| {
                let ty = self.checker.get_type_of_symbol(symbol);
                !holds_only_data(self.checker, ty, 4)
            })
        } else {
            // an object pattern that is not rest (`{ onDone }: Opts`) holds what its parameter's type holds: the engine gives a binding pattern the type of
            // its parameter (`get_type_of_node`). An array pattern and every rest parameter are not capable.
            name.kind() == Kind::ObjectBindingPattern && !parameter.is_rest() && {
                let ty = self.checker.get_type_at_location(name);
                !holds_only_data(self.checker, ty, 4)
            }
        };
        (
            Param {
                rest: parameter.is_rest(),
                named,
                default,
                assigned: false,
                capable,
                uses: Uses::default(),
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
            FlowKind::Jsx => Some(
                vec![Arg {
                    value: self.jsx_attributes(call.call),
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

    /// The one argument of a JSX site (`08-tc4` §2): an `Object` of the attributes of `tag`, a `JsxOpeningElement` or a `JsxSelfClosingElement`, and last
    /// `children` when the element (the opening element's parent) has any child. The attributes are found by walking the tag's children, since the engine's
    /// `attributes` is not asked outside `view`.
    fn jsx_attributes(&mut self, tag: P<Node>) -> Expr {
        let text = self.file.node.text();
        let mut list = None;
        tag.for_each_child(&mut |child| {
            if child.kind() == Kind::JsxAttributes {
                list = Some(child);
            }
            list.is_some()
        });
        let mut attributes = Vec::new();
        if let Some(list) = list {
            list.for_each_child(&mut |child| {
                attributes.push(child);
                false
            });
        }
        let mut parts = Vec::new();
        for attribute in attributes {
            match attribute.kind() {
                Kind::JsxAttribute => {
                    let name = attribute
                        .name()
                        .and_then(|name| own_span(text, name).slice(text));
                    let value = match initializer_of(attribute) {
                        // `<P label />` is `true`, a string is a string, an element is an element
                        None => Expr::Data,
                        Some(value) => match value.kind() {
                            Kind::JsxExpression => expression_of(value)
                                .map_or(Expr::Unknown, |inner| self.expr_of(inner)),
                            Kind::StringLiteral
                            | Kind::JsxElement
                            | Kind::JsxSelfClosingElement
                            | Kind::JsxFragment => Expr::Data,
                            _ => Expr::Unknown,
                        },
                    };
                    parts.push(match name {
                        Some(name) => Part::Named(name.into(), value),
                        None => Part::Unnamed,
                    });
                }
                Kind::JsxSpreadAttribute => parts.push(Part::Spread(
                    expression_of(attribute).map_or(Expr::Unknown, |inner| self.expr_of(inner)),
                )),
                _ => parts.push(Part::Unnamed),
            }
        }
        let mut has_children = false;
        if tag.kind() == Kind::JsxOpeningElement
            && let Some(element) = tag.parent()
        {
            element.for_each_child(&mut |child| {
                has_children |= match child.kind() {
                    Kind::JsxOpeningElement | Kind::JsxClosingElement => false,
                    // text of white space with a line break in it is not rendered: `containsOnlyTriviaWhiteSpaces` (`ast::is_whitespace_only_jsx_text`);
                    // spaces on one line are (`<P> </P>`), as React renders them
                    Kind::JsxText => !ast::is_whitespace_only_jsx_text(child),
                    // `{/* note */}` has no expression
                    Kind::JsxExpression => expression_of(child).is_some(),
                    _ => true,
                };
                has_children
            });
        }
        if has_children {
            parts.push(Part::Named("children".into(), Expr::Unknown));
        }
        Expr::Object(parts.into())
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

    /// The symbol of a name. For a shorthand property (`{ f }`) the node's own symbol is the property, so the variable it names is asked for instead.
    fn symbol_of(&mut self, name: P<Node>) -> Option<P<Symbol>> {
        if name.kind() == Kind::ShorthandPropertyAssignment {
            self.checker
                .get_shorthand_assignment_value_symbol(Some(name))
        } else {
            self.checker.get_symbol_at_location_exported(name)
        }
    }

    /// The place in this def's parameters of the one `name` stands for: its symbol's value declaration is one of `self.params`. The symbol is not followed
    /// through an alias: a parameter is never an import.
    fn param_index(&mut self, name: P<Node>) -> Option<u32> {
        let symbol = self.symbol_of(name)?;
        let declaration = symbol.value_declaration()?;
        let index = self.params.iter().position(|&param| param == declaration)?;
        Some(u32::try_from(index).expect("a def has fewer than 2^32 parameters"))
    }

    /// The variable `name` stands for: its symbol's value declaration is a variable, a parameter or a binding element of this file with a plain name, and the
    /// def that declares it is this def or one around it. An import (`SymbolFlags::Alias`, `export { x } from` too) is followed to what it names
    /// (`imported_var`): the variable of another project file that is at the top of it. The scope rule is what an `Expr::Var` of this def's table may name.
    fn var_of(&mut self, name: P<Node>) -> Option<VarId> {
        self.variable_named(name, true)
    }

    /// `var_of` without the scope rule, for a name the placement of a use meets inside this def's node (`const { a } = x`, `const o = x`, `y = x`): the
    /// declaration is inside the node, so the def that declares it is this def or one nested in it, and the variable is that def's row (`08-tc4` §10, step 4
    /// corrected at 4b, point 3). A name declared outside this def's node is named as `var_of` names it.
    fn declared_var(&mut self, name: P<Node>) -> Option<VarId> {
        self.variable_named(name, false)
    }

    /// `var_of` and `declared_var`: `scoped` asks that the declaring def is this def or one around it.
    fn variable_named(&mut self, name: P<Node>, scoped: bool) -> Option<VarId> {
        let symbol = self.symbol_of(name)?;
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
        if scoped {
            let mut around = Some(self.def);
            loop {
                let node = around?;
                if node == holder {
                    break;
                }
                around = node.parent();
            }
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

    /// The function `call` binds when it is `f.bind()` or `f.bind(thisArg)` of `Function.prototype.bind`: `f`'s node. `None` for any other call. Three
    /// conditions: the callee is a property access spelled `bind`; there are no arguments, or one that is not a spread; and the checker's resolved signature
    /// is a method `bind` of the interface `Function`, `CallableFunction` or `NewableFunction`, declared in a file that is not project code (the library's).
    fn bound_function(&mut self, call: P<Node>) -> Option<P<Node>> {
        let callee = expression_of(call)?;
        if callee.kind() != Kind::PropertyAccessExpression {
            return None;
        }
        let name = callee.name()?;
        if spelled(self.file.node.text(), name).as_deref() != Some("bind") {
            return None;
        }
        match arguments_of(call) {
            [] => {}
            [this] if this.kind() != Kind::SpreadElement => {}
            _ => return None,
        }
        let declaration = self
            .checker
            .get_resolved_signature_exported(call)
            .declaration()?;
        let file = ast::get_source_file_of_node(declaration)?;
        let declared_in = File {
            node: file,
            _sem: PhantomData,
            _one_thread: PhantomData,
        };
        if self.sem.is_project_code(declared_in) {
            return None;
        }
        let method = declaration.name()?;
        if spelled(file.text(), method).as_deref() != Some("bind") {
            return None;
        }
        let interface = declaration
            .parent()
            .filter(|parent| parent.kind() == Kind::InterfaceDeclaration)?;
        let interface_name = spelled(file.text(), interface.name()?)?;
        matches!(
            interface_name.as_str(),
            "Function" | "CallableFunction" | "NewableFunction"
        )
        .then(|| expression_of(callee))?
    }

    /// Whether the identifier is the name `undefined` and the checker's global one: its symbol has no declaration in any file (a local `const undefined = f`
    /// has one, and is a variable like any other).
    fn is_global_undefined(&mut self, node: P<Node>) -> bool {
        if spelled(self.file.node.text(), node).as_deref() != Some("undefined") {
            return false;
        }
        self.symbol_of(node)
            .is_some_and(|symbol| symbol.declarations().is_empty())
    }

    /// `x.k` or `x?.k` as a member of what `x` is: `Unknown` for a private name (`x.#k`), which is no name.
    fn member_of_access(&mut self, access: P<Node>) -> Expr {
        let name = access.name().and_then(|name| self.property_name(name));
        match (name, expression_of(access)) {
            (Some(name), Some(object)) => member(self.expr_of(object), name),
            _ => Expr::Unknown,
        }
    }

    /// One property of an object literal, as a part (`08-tc4` §2). A getter, a setter and a computed key have no name the table can use: `Unnamed`.
    fn part_of(&mut self, property: P<Node>) -> Part {
        let named =
            |reader: &Reader<'_, '_>| property.name().and_then(|name| reader.property_name(name));
        match property.kind() {
            Kind::PropertyAssignment => match named(self) {
                Some(name) => Part::Named(
                    name,
                    initializer_of(property).map_or(Expr::Unknown, |value| self.expr_of(value)),
                ),
                None => Part::Unnamed,
            },
            // a method of a literal is a def of the outline (D9), so `expr_of` gives its `Def`
            Kind::ShorthandPropertyAssignment | Kind::MethodDeclaration => match named(self) {
                Some(name) => Part::Named(name, self.expr_of(property)),
                None => Part::Unnamed,
            },
            Kind::SpreadAssignment => Part::Spread(
                expression_of(property).map_or(Expr::Unknown, |inner| self.expr_of(inner)),
            ),
            _ => Part::Unnamed,
        }
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
            // a shorthand property `{ f }` names a variable as an identifier does (`symbol_of`)
            Kind::Identifier
            | Kind::PropertyAccessExpression
            | Kind::ShorthandPropertyAssignment => {
                let is_access = node.kind() == Kind::PropertyAccessExpression;
                if !is_access && self.is_global_undefined(node) {
                    return Expr::Undefined;
                }
                if !is_access && let Some(index) = self.param_index(node) {
                    return Expr::Param(index);
                }
                let (functions, classes) =
                    self.sem
                        .functions_named(&mut *self.checker, node, self.style);
                if classes > 0 || functions.is_empty() {
                    // where no function is named, an access is a member of what its object is (`x.k`), a name a variable
                    return if is_access {
                        self.member_of_access(node)
                    } else {
                        self.var_of(node).map_or(Expr::Unknown, Expr::Var)
                    };
                }
                // one symbol can stand for one function twice (a script's function merged with a library global of the same name, the DOM's `open`): `one_of` keeps it once
                one_of(
                    functions
                        .into_iter()
                        .map(|function| Expr::Def(self.sem.def_id(function))),
                )
            }
            // `x["k"]`: a member when the key is a string; `x[i]` is an index, and arrays are not modelled
            Kind::ElementAccessExpression => {
                let key = ElementAccess::cast(node)
                    .map(ElementAccess::argument)
                    .filter(|argument| {
                        matches!(
                            argument.kind(),
                            Kind::StringLiteral | Kind::NoSubstitutionTemplateLiteral
                        )
                    })
                    .and_then(|argument| self.property_name(argument));
                match (key, expression_of(node)) {
                    (Some(key), Some(object)) => member(self.expr_of(object), key),
                    _ => Expr::Unknown,
                }
            }
            Kind::ObjectLiteralExpression => {
                let mut parts = Vec::new();
                for &property in properties_of(node) {
                    parts.push(self.part_of(property));
                }
                Expr::Object(parts.into())
            }
            // `f.bind(x)` with nothing or one argument that is `this`: the same function with `this` fixed, so `f`'s value (`07-tc3` §6). An argument after
            // `this` shifts the parameters, so it is not. Only `Function.prototype.bind`: a `bind` of a class or object of its own is a call like any other.
            Kind::CallExpression => match self.bound_function(node) {
                Some(target) => self.expr_of(target),
                None => Expr::Unknown,
            },
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
            | Kind::DeleteExpression
            | Kind::JsxElement
            | Kind::JsxSelfClosingElement
            | Kind::JsxFragment => Expr::Data,
            // `void x` is `undefined` whatever `x` is: nothing was passed, so a default applies (the review before TC3, §1.1)
            Kind::VoidExpression => Expr::Undefined,
            _ => Expr::Unknown,
        }
    }
}

/// Whether a variable declaration is in a `for … of` or `for … in`, in a `catch`, or ambient: its initializer (if any) is not its value.
fn is_open_by_form(declaration: P<Node>) -> bool {
    let parent = declaration.parent();
    let in_loop = parent
        .and_then(|list| list.parent())
        .is_some_and(|statement| {
            matches!(
                statement.kind(),
                Kind::ForOfStatement | Kind::ForInStatement
            )
        });
    let in_catch = parent.is_some_and(|parent| parent.kind() == Kind::CatchClause);
    let ambient = ast::get_combined_node_flags(declaration).intersects(NodeFlags::Ambient);
    in_loop || in_catch || ambient
}

/// The only maker of `Expr::Member`: `Unknown` when the object is (a member of nothing known is nothing known).
fn member(object: Expr, name: Box<str>) -> Expr {
    match object {
        Expr::Unknown => Expr::Unknown,
        object => Expr::Member(Box::new(object), name),
    }
}

/// The only maker of `Expr::Omit`: `Unknown` when the object is.
fn omit(object: Expr, names: Box<[Box<str>]>) -> Expr {
    match object {
        Expr::Unknown => Expr::Unknown,
        object => Expr::Omit(Box::new(object), names),
    }
}

/// Whether a function-like node is between `node` and `root`: what makes a variable declared under `root` that function's own and not the file's.
fn inside_a_function(node: P<Node>, root: P<Node>) -> bool {
    let mut up = node.parent();
    while let Some(parent) = up {
        if parent == root {
            return false;
        }
        if matches!(
            parent.kind(),
            Kind::ArrowFunction
                | Kind::FunctionExpression
                | Kind::FunctionDeclaration
                | Kind::MethodDeclaration
                | Kind::GetAccessor
                | Kind::SetAccessor
                | Kind::Constructor
        ) {
            return true;
        }
        up = parent.parent();
    }
    false
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

/// What `place` has found of one variable so far; `uses_of` makes the row's `Uses` of it.
#[derive(Default)]
struct Found {
    escapes: bool,
    passed: Vec<(DefId, u32)>,
    names: Vec<VarId>,
    receiver: bool,
}

/// `items` in order, without a repeat.
fn sorted<T: Ord>(mut items: Vec<T>) -> Vec<T> {
    items.sort();
    items.dedup();
    items
}

/// `node` and every wrapper above it that only wraps it (`(x)`, `x as T`, `x!`, `x satisfies T`, `<T>x`): the topmost, which is `node` when it has none.
fn wrapped(node: P<Node>) -> P<Node> {
    let mut top = node;
    while let Some(parent) = top.parent()
        && let Some(wrapper) = Wrapper::cast(parent)
        && wrapper.inner() == Some(top)
    {
        top = parent;
    }
    top
}

/// Whether the value of `node` is only tested, never kept (`08-tc4` §5, point 3): it is the condition of an `if`, `while`, `do`, `for` or `c ? a : b`, the operand of
/// `!` or `typeof`, a side of `===`, `!==`, `==`, `!=`, `instanceof` or `in`, or the expression of an expression statement. A `for`'s three expressions are
/// all evaluated for their effect or their truth; none of them keeps its value.
fn is_tested(node: P<Node>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    match parent.kind() {
        Kind::IfStatement
        | Kind::WhileStatement
        | Kind::DoStatement
        | Kind::ForStatement
        | Kind::ExpressionStatement
        | Kind::TypeOfExpression => true,
        Kind::PrefixUnaryExpression => PrefixUnary::cast(parent)
            .is_some_and(|unary| unary.operator() == Kind::ExclamationToken),
        Kind::ConditionalExpression => {
            Conditional::cast(parent).is_some_and(|conditional| conditional.condition() == node)
        }
        Kind::BinaryExpression => Binary::cast(parent).is_some_and(|binary| {
            matches!(
                binary.operator(),
                Kind::EqualsEqualsEqualsToken
                    | Kind::ExclamationEqualsEqualsToken
                    | Kind::EqualsEqualsToken
                    | Kind::ExclamationEqualsToken
                    | Kind::InstanceOfKeyword
                    | Kind::InKeyword
            )
        }),
        _ => false,
    }
}

/// Every reference under `container` (its parameters, its body, and the defs and anonymous functions written in it) to a variable whose name is in `wanted`:
/// each `Identifier` that is not the name a declaration gives and not part of a `typeof x` type, with its symbol; a shorthand property `{ x }` as the property
/// node itself, with the symbol of the variable it names (`get_shorthand_assignment_value_symbol`, since its own symbol is the property). The name is compared
/// before the checker is asked, so the walk costs the checker only the names of capable variables. Same walk, same stack, as `written_identifiers`.
fn referenced_identifiers(
    checker: &mut Checker,
    container: P<Node>,
    wanted: &HashSet<&str>,
) -> HashMap<P<Symbol>, Vec<P<Node>>> {
    let mut found: HashMap<P<Symbol>, Vec<P<Node>>> = HashMap::new();
    let mut pending = vec![container];
    while let Some(node) = pending.pop() {
        let symbol = match node.kind() {
            Kind::Identifier
                if NameToken::cast(node).is_some_and(|name| wanted.contains(name.value()))
                    && ast::get_declaration_from_name(Some(node)).is_none()
                    && !is_in_type_query(node) =>
            {
                checker.get_symbol_at_location_exported(node)
            }
            Kind::ShorthandPropertyAssignment
                if node
                    .name()
                    .and_then(NameToken::cast)
                    .is_some_and(|name| wanted.contains(name.value())) =>
            {
                checker.get_shorthand_assignment_value_symbol(Some(node))
            }
            _ => None,
        };
        if let Some(symbol) = symbol {
            found.entry(symbol).or_default().push(node);
        }
        node.for_each_child(&mut |child| {
            pending.push(child);
            false
        });
    }
    found
}

/// Whether the identifier is the name in a type `typeof x` or `typeof x.y` (a type query reads no value).
fn is_in_type_query(identifier: P<Node>) -> bool {
    let mut up = identifier.parent();
    while let Some(node) = up {
        match node.kind() {
            Kind::QualifiedName => up = node.parent(),
            Kind::TypeQuery => return true,
            _ => return false,
        }
    }
    false
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
