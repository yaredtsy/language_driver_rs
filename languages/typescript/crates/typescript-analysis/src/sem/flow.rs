//! `Sem::flow`: reading a def's body into a `Flow` (`crate::flow`), the one place the table is made.
//!
//! The table is read **once** per def and kept for as long as the program is loaded, so that a later question ("what is this callee, what is each argument") is
//! asked of plain data and not of the checker. Nothing here lets an engine pointer out: `Flow` holds numbers and owned values, and a def is named by a `DefId`,
//! a number of this program (`Flows`), which `Sem::func` turns back into a `Func` only for the program that made it.

use std::sync::atomic::{AtomicU32, Ordering};

use indexmap::IndexSet;
use tsrs_ast::Symbol;
use tsrs_checker::{Type, TypeFlags, astnav};

use super::collect::{CallNode, SiteKind, unwrap_expression};
use super::handed::HandOvers;
use super::view::{
    Binary, Conditional, Parameter, arguments_of, expression_of, parameters_of, spelled,
};
use super::*;
use crate::flow::{AnonFlow, Arg, DefId, Expr, Flow, FlowKind, HandedFlow, Param, SiteFlow};

/// The tables of one program, and the numbers they are written in.
pub(super) struct Flows {
    /// This program's number: from `PROGRAMS`, taken when the `Sem` is made.
    program: u32,
    /// The files that have a number: a file's number is its place here, given on first use. (The prompt of this step said `P<Node>`; a `P<SourceFile>` is what
    /// a `File` holds, so the file comes back from its number without a downcast of the engine's.)
    files: RefCell<IndexSet<P<SourceFile>>>,
    memo: RefCell<HashMap<DefId, Arc<Flow>>>,
}

/// The next program's number. It never reaches an output: it only tells two programs' numbers apart.
static PROGRAMS: AtomicU32 = AtomicU32::new(0);

impl Flows {
    pub(super) fn new() -> Flows {
        Flows {
            program: PROGRAMS.fetch_add(1, Ordering::Relaxed),
            files: RefCell::new(IndexSet::new()),
            memo: RefCell::new(HashMap::new()),
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
        let params = self
            .params
            .clone()
            .into_iter()
            .map(|parameter| self.param(parameter))
            .collect();
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
        }
    }

    /// One row of `Flow::params`: what `06-tc2` §2 needs to know of the parameter `node`.
    fn param(&mut self, node: P<Node>) -> Param {
        let Some(parameter) = Parameter::cast(node) else {
            return Param {
                rest: false,
                named: false,
                default: None,
                assigned: false,
                capable: false,
            };
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
        let assigned = capable
            && symbol.is_some_and(|symbol| {
                spelled(self.file.node.text(), name).is_some_and(|text| {
                    !writes_of(self.checker, self.file.node, symbol, &text, self.def).is_empty()
                })
            });
        Param {
            rest: parameter.is_rest(),
            named,
            default,
            assigned,
            capable,
        }
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
                    return Expr::Unknown;
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

/// The nodes under `container` where the text `name` stands as a whole word, as the engine's `get_possible_symbol_reference_nodes` finds them (a private function
/// of `tsrs_checker`, and its search takes the first hit's offset from the start of the container, not of the file: `ENGINE.md`), in order: the token each hit touches,
/// when it is not the file itself. A candidate only: a word in a comment or a string, or another symbol of the same name, is in it too. The container's own
/// leading trivia is searched with it. Used for a parameter's writes (TC2) and later a local's (TC3).
fn names_in(file: P<SourceFile>, name: &str, container: P<Node>) -> Vec<P<Node>> {
    let mut found = Vec::new();
    let text = file.text();
    let bytes = text.as_bytes();
    let needle = name.as_bytes();
    if needle.is_empty() {
        return found;
    }
    let end = (container.end().source_offset().unwrap_or(0) as usize).min(bytes.len());
    let mut at = container.pos().source_offset().unwrap_or(0) as usize;
    while at < end {
        let Some(offset) = tsrs_core::index_bytes(&bytes[at..end], needle) else {
            break;
        };
        // the engine's bug, fixed: the hit is `offset` past `at`, not at `offset`
        let hit = at + offset;
        let after = hit + needle.len();
        let word = |byte: &u8| tsrs_scanner::is_identifier_part(u32::from(*byte));
        if !bytes[..hit].last().is_some_and(word) && !bytes.get(after).is_some_and(word) {
            let node = astnav::get_touching_property_name(file, hit as u32);
            if node != file.as_node() {
                found.push(node);
            }
        }
        at = after;
    }
    found
}

/// The identifiers under `container` that write `symbol`, whose name is written `name`: a candidate of `names_in` that is an identifier of that symbol and
/// that `ast::is_write_access` says is written. Used for a parameter's writes (TC2) and later a local's (TC3). The declaration's own name is never one of
/// them: `is_write_access` says `Read` for a name whose parent is a `Parameter`. The write test is asked before the checker, since it is syntax only.
fn writes_of(
    checker: &mut Checker,
    file: P<SourceFile>,
    symbol: P<Symbol>,
    name: &str,
    container: P<Node>,
) -> Vec<P<Node>> {
    let mut writes: Vec<P<Node>> = names_in(file, name, container)
        .into_iter()
        .filter(|node| {
            node.kind() == Kind::Identifier
                // syntax only, so it comes first: most places that spell the name read it
                && ast::is_write_access(*node)
                && checker.get_symbol_at_location_exported(*node) == Some(symbol)
        })
        .collect();
    // a position in a comment can touch the token after it, which another hit of the same word touches too
    writes.dedup();
    writes
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
