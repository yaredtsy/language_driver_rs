//! `Sem::flow`: reading a def's body into a `Flow` (`crate::flow`), the one place the table is made.
//!
//! The table is read **once** per def and kept for as long as the program is loaded, so that a later question ("what is this callee, what is each argument") is
//! asked of plain data and not of the checker. Nothing here lets an engine pointer out: `Flow` holds numbers and owned values, and a def is named by a `DefId`,
//! a number of this program (`Flows`), which `Sem::func` turns back into a `Func` only for the program that made it.

use std::sync::atomic::{AtomicU32, Ordering};

use indexmap::IndexSet;

use super::collect::{CallNode, SiteKind, unwrap_expression};
use super::handed::HandOvers;
use super::view::{Binary, Conditional, Parameter, arguments_of, expression_of, parameters_of};
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
                anon_at: body
                    .anons
                    .iter()
                    .enumerate()
                    .map(|(index, &span)| (span, index as u32))
                    .collect(),
            };
            reader.read(&body, &hand_overs, node)
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
    /// The place of each anonymous function of the body, by its span.
    anon_at: HashMap<Span, u32>,
}

impl Reader<'_, '_> {
    fn read(&mut self, body: &Body, hand_overs: &HandOvers<'_>, node: P<Node>) -> Flow {
        let params = parameters_of(node)
            .iter()
            .filter(|&&parameter| !ast::is_this_parameter(parameter))
            .map(|&parameter| Param {
                rest: Parameter::cast(parameter).is_some_and(Parameter::is_rest),
            })
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
                let (functions, classes) =
                    self.sem
                        .functions_named(&mut *self.checker, node, self.style);
                if classes > 0 || functions.is_empty() {
                    return Expr::Unknown;
                }
                // one symbol can have several declarations of one function (overloads, a script's function merged with a library's global):
                // `one_of` keeps each function once
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
