use super::{BoundTo, EntryKind, Shape, qname};
use crate::stable::{StableDef, StableType};
use analyzer_core::edit::MarkerStyle;
use analyzer_core::graph::Entry;
use analyzer_core::outline::DefKind;
use ruff_db::files::File;
use ruff_db::parsed::{ParsedModuleRef, parsed_module};
use ruff_db::source::source_text;
use ruff_python_ast::visitor::{Visitor, walk_stmt};
use ruff_python_ast::{AnyNodeRef, ExprName, Stmt, StmtFunctionDef};
use ruff_text_size::{Ranged, TextRange, TextSize};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use ty_module_resolver::file_to_module;

use ty_project::Db as _;
use ty_project::glob::IncludeResult;
use ty_python_core::ProgramFile;
use ty_python_core::definition::{Definition, DefinitionKind};
use ty_python_semantic::types::Type;
use ty_python_semantic::types::analyzer_ext;
use ty_python_semantic::{
    Db as _, HasDefinition, ImportAliasResolution, ResolvedDefinition, definitions_for_name,
};
use ty_python_semantic::{ProgramEnvironment, SemanticModel};

/// What the outline calls a def: the one naming rule a request and an answer share (D20).
#[derive(Clone, Debug)]
pub(crate) struct OutlineName {
    pub qualname: String,
    /// Which of the defs of the file with this qualname it is, 0-based, in source order.
    pub occurrence: u16,
    /// What the outline says it is: `Method` for a method, not `Function`.
    pub kind: DefKind,
}

/// The outline's names for the defs of one file.
#[derive(Default)]
pub(crate) struct NamedDefs {
    /// The start of a def's name to what the outline calls it.
    by_name: FxHashMap<u32, OutlineName>,
    /// A qualname to the starts of the names of the defs that have it, in source order: `occurrence` indexes it. Only looked up by key.
    starts: FxHashMap<String, Vec<u32>>,
}

impl NamedDefs {
    fn new(text: &str, marker: MarkerStyle<'_>) -> Self {
        let mut seen: FxHashMap<&str, u16> = FxHashMap::default();
        let outline = python_syntax::outline(text, marker);
        let mut named = NamedDefs::default();
        for def in &outline.defs {
            let n = seen.entry(def.qualname.as_str()).or_insert(0);
            named.by_name.insert(
                def.name_range.start,
                OutlineName {
                    qualname: def.qualname.clone(),
                    occurrence: *n,
                    kind: def.kind.clone(),
                },
            );
            *n += 1;
            named
                .starts
                .entry(def.qualname.clone())
                .or_default()
                .push(def.name_range.start);
        }
        named
    }

    /// Where the name of the def `entry` asks for starts, if the outline has that many defs with its qualname.
    fn start_of(&self, entry: &Entry<'_>) -> Option<u32> {
        self.starts
            .get(entry.qualname)?
            .get(usize::from(entry.occurrence))
            .copied()
    }
}

impl<'db> super::Sem<'db> {
    pub(crate) fn definition_of(
        &self,
        model: &SemanticModel<'db>,
        d: &StmtFunctionDef,
    ) -> Definition<'db> {
        d.definition(model)
    }
    /// A function in `class`, by what ty makes of it on an instance: the same order as `member`.
    #[allow(dead_code)] // inline mode is the fixture suite's cross-check and is not reachable from the public API
    pub(crate) fn entry_kind(&self, class: Definition<'db>, func: Definition<'db>) -> EntryKind {
        let name = self.function_name(func);
        if self.property_parts(class, &name).is_some() {
            return EntryKind::Method; // a property's getter runs on an object
        }
        let file = class.program_file(self.db);
        match self
            .member(self.instance_type(class), &name, file)
            .map(|t| (t, self.shape(t, file)))
        {
            Some((t, Shape::BoundMethod { .. })) => match self.bound_to(t) {
                BoundTo::Class => EntryKind::ClassMethod,
                _ => EntryKind::Method,
            },
            Some((_, Shape::Function(_))) => EntryKind::StaticMethod, // no receiver on an instance
            _ => EntryKind::Method, // ty can't tell (an unknown decorator): a method
        }
    }

    pub(crate) fn def_range(&self, def: Definition<'db>) -> TextRange {
        def.full_range(self.db, &self.parsed(def)).range()
    }

    #[allow(dead_code)] // inline mode is the fixture suite's cross-check and is not reachable from the public API
    pub(crate) fn function_name(&self, def: Definition<'db>) -> String {
        let parsed = self.parsed(def);
        self.function_node(def, &parsed)
            .map_or_else(String::new, |f| f.name.to_string())
    }

    /// The docstring `ID:` of a project `def` or `class`. The one reader `scan` uses.
    pub(crate) fn docstring_id(&self, def: Definition<'db>) -> Option<String> {
        if !self.is_project(def) {
            return None;
        }
        let parsed = self.parsed(def);
        let body = match def.kind(self.db) {
            DefinitionKind::Function(f) => &f.node(&parsed).body,
            DefinitionKind::Class(c) => &c.node(&parsed).body,
            _ => return None,
        };
        // the docstring is the first statement, when it is a string literal on its own
        let doc = body
            .first()?
            .as_expr_stmt()?
            .value
            .as_string_literal_expr()?
            .value
            .to_str();
        python_syntax::docstring_marker(doc, self.marker)
    }

    pub(crate) fn is_class(&self, def: Definition<'db>) -> bool {
        matches!(def.kind(self.db), DefinitionKind::Class(_))
    }

    /// A definition's identity outside this request: its docstring `ID:` when it
    /// is project code that has one, else where it is (module, qualified name, nth).
    pub(crate) fn stable_def(&self, def: Definition<'db>) -> StableDef {
        let parsed = self.parsed(def);
        let start = match def.kind(self.db) {
            DefinitionKind::Function(f) => f.node(&parsed).range().start(),
            DefinitionKind::Class(c) => c.node(&parsed).range().start(),
            _ => self.def_position(def).1,
        };
        if let Some(id) = self.docstring_id(def) {
            return StableDef::Marked(id);
        }
        let module = file_to_module(self.db, def.program_file(self.db).resolver_file(self.db))
            .map(|m| m.name(self.db).to_string())
            .unwrap_or_default();
        let full = self.qname(def);
        let qname = full
            .strip_prefix(&format!("{module}."))
            .unwrap_or(&full)
            .to_string();
        let nth = self
            .nth_of(def.file(self.db), &parsed)
            .get(&start)
            .copied()
            .unwrap_or(0);
        StableDef::Path { module, qname, nth }
    }

    /// The text of `file`, as the engine read it.
    pub(crate) fn source_text(&self, file: File) -> String {
        source_text(self.db, file).as_str().to_owned()
    }

    /// The outline's names for the defs of `file`, read once.
    fn named_defs(&self, file: File) -> Rc<NamedDefs> {
        if let Some(found) = self.named.borrow().get(&file) {
            return found.clone();
        }
        let table = Rc::new(NamedDefs::new(&self.source_text(file), self.marker));
        self.named.borrow_mut().insert(file, table.clone());
        table
    }

    /// The outline's name, occurrence and kind for a def or class: `None` for any other kind of definition, or when the outline
    /// has no def at the start of its name.
    pub(crate) fn outline_name(&self, def: Definition<'db>) -> Option<OutlineName> {
        let parsed = self.parsed(def);
        let start = match def.kind(self.db) {
            DefinitionKind::Function(f) => f.node(&parsed).name.range().start(),
            DefinitionKind::Class(c) => c.node(&parsed).name.range().start(),
            _ => return None,
        };
        self.named_defs(def.file(self.db))
            .by_name
            .get(&u32::from(start))
            .cloned()
    }

    /// `nth` of every `def` and `class` in a file, by start offset: its place
    /// among the defs with the same qualified name, in source order.
    fn nth_of(&self, file: File, parsed: &ParsedModuleRef) -> Rc<FxHashMap<TextSize, u16>> {
        if let Some(found) = self.nth.borrow().get(&file) {
            return found.clone();
        }
        struct Walker {
            path: Vec<String>,
            seen: FxHashMap<String, u16>,
            out: FxHashMap<TextSize, u16>,
        }
        impl<'a> Visitor<'a> for Walker {
            fn visit_stmt(&mut self, stmt: &'a Stmt) {
                let name = match stmt {
                    Stmt::FunctionDef(f) => Some(f.name.to_string()),
                    Stmt::ClassDef(c) => Some(c.name.to_string()),
                    _ => None,
                };
                let Some(name) = name else {
                    return walk_stmt(self, stmt);
                };
                self.path.push(name);
                let n = self.seen.entry(self.path.join(".")).or_insert(0);
                self.out.insert(stmt.range().start(), *n);
                *n += 1;
                walk_stmt(self, stmt);
                self.path.pop();
            }
        }
        let mut w = Walker {
            path: Vec::new(),
            seen: FxHashMap::default(),
            out: FxHashMap::default(),
        };
        w.visit_body(&parsed.syntax().body);
        let out = Rc::new(w.out);
        self.nth.borrow_mut().insert(file, out.clone());
        out
    }

    /// A type's identity by structure, or `None` when it can't be written
    /// one-to-one (then the key must not hold it).
    pub(crate) fn stable_type(&self, ty: Type<'db>, file: ProgramFile<'db>) -> Option<StableType> {
        let env = ProgramEnvironment::from_file(file);
        match ty {
            Type::NominalInstance(_) => {
                let Shape::Instance(class) = self.shape(ty, file) else {
                    return None;
                };
                let args = analyzer_ext::type_args(self.db, &env, ty)?
                    .into_iter()
                    .map(|a| self.stable_type(a, file))
                    .collect::<Option<Vec<_>>>()?;
                Some(StableType::Instance {
                    class: self.stable_def(class),
                    args,
                })
            }
            Type::Callable(_) => {
                let (params, ret) = analyzer_ext::callable_parts(self.db, ty)?;
                let params = match params {
                    Some(ps) => Some(
                        ps.into_iter()
                            .map(|p| self.stable_type(p, file))
                            .collect::<Option<Vec<_>>>()?,
                    ),
                    None => None,
                };
                Some(StableType::Callable {
                    params,
                    ret: Box::new(self.stable_type(ret, file)?),
                })
            }
            _ => None,
        }
    }

    /// The name of a module literal type.
    pub(crate) fn module_name(&self, ty: Type<'db>) -> String {
        match ty {
            Type::ModuleLiteral(m) => m.module(self.db).name(self.db).to_string(),
            _ => String::new(),
        }
    }

    pub(crate) fn is_project(&self, def: Definition<'db>) -> bool {
        let file = def.file(self.db);
        let Some(path) = file.path(self.db).as_system_path() else {
            return false;
        };
        matches!(
            self.db.project().is_file_included(self.db, path),
            IncludeResult::Included { .. }
        )
    }
    pub(crate) fn parsed(&self, def: Definition<'db>) -> ParsedModuleRef {
        parsed_module(self.db, def.python_file(self.db)).load(self.db)
    }
    /// The `def` statement of a function. `parsed` must be `self.parsed(def)`.
    pub(crate) fn function_node<'m>(
        &self,
        def: Definition<'db>,
        parsed: &'m ParsedModuleRef,
    ) -> Option<&'m StmtFunctionDef> {
        match def.kind(self.db) {
            DefinitionKind::Function(f) => Some(f.node(parsed)),
            _ => None,
        }
    }

    /// If `name` refers to `def` statements with decorators, those defs.
    pub(crate) fn decorated_defs(
        &self,
        model: &SemanticModel<'db>,
        name: &ExprName,
    ) -> Option<Vec<Definition<'db>>> {
        let found = definitions_for_name(
            model,
            name.id.as_str(),
            AnyNodeRef::ExprName(name),
            ImportAliasResolution::ResolveAliases,
        );
        let mut defs = Vec::new();
        for r in found {
            let ResolvedDefinition::Definition(def) = r else {
                return None;
            };
            let parsed = self.parsed(def);
            let node = self.function_node(def, &parsed)?; // not a def: not this rule
            if node.decorator_list.is_empty() {
                return None; // undecorated: ty's answer is the def anyway
            }
            defs.push(def);
        }
        (!defs.is_empty()).then_some(defs)
    }

    /// The def `entry` names: the `occurrence`th def of `file` that the outline gives `qualname`, found in the syntax tree by where its name
    /// starts (D20). A def or a class, at any depth, in any block. `None` when the outline has fewer defs with that name.
    pub(crate) fn entry(&self, file: File, entry: &Entry<'_>) -> Option<Definition<'db>> {
        let start = self.named_defs(file).start_of(entry)?;
        let program_file = self.db.program_file(file);
        let parsed = parsed_module(self.db, program_file.python_file(self.db)).load(self.db);
        let model = SemanticModel::new(self.db, program_file);

        /// Finds the `def` or `class` whose name starts at `start`.
        struct Finder<'a> {
            start: TextSize,
            found: Option<&'a Stmt>,
        }
        impl<'a> Visitor<'a> for Finder<'a> {
            fn visit_stmt(&mut self, stmt: &'a Stmt) {
                if self.found.is_some() {
                    return;
                }
                let name_start = match stmt {
                    Stmt::FunctionDef(f) => Some(f.name.range().start()),
                    Stmt::ClassDef(c) => Some(c.name.range().start()),
                    _ => None,
                };
                if name_start == Some(self.start) {
                    self.found = Some(stmt);
                } else {
                    walk_stmt(self, stmt);
                }
            }
        }
        let mut finder = Finder {
            start: TextSize::from(start),
            found: None,
        };
        finder.visit_body(&parsed.syntax().body);
        match finder.found? {
            Stmt::FunctionDef(f) => Some(f.definition(&model)),
            Stmt::ClassDef(c) => Some(c.definition(&model)),
            _ => None,
        }
    }

    /// The definition's file, relative to the project root, with `/`. A file outside the root (library code) keeps its full path.
    pub(crate) fn relative_file(&self, def: Definition<'db>) -> String {
        let path = def.file(self.db).path(self.db);
        let root = self.db.project().root(self.db);
        let shown = match path
            .as_system_path()
            .and_then(|p| p.strip_prefix(root).ok())
        {
            Some(relative) => relative.to_string(),
            None => path.to_string(),
        };
        shown.replace('\\', "/")
    }

    /// Where the definition lives: its file path, then the offset of its `def`.
    /// A stable order for nodes that the walk can reach in any order.
    pub(crate) fn def_position(&self, def: Definition<'db>) -> (String, TextSize) {
        let path = def.file(self.db).path(self.db).to_string();
        let parsed = self.parsed(def);
        // A `def` keeps its node's start: call sites are stored relative to it.
        // Anything else (a class) by its definition's range: it used to be 0 for
        // every class, so sorting classes by position did nothing.
        let start = match self.function_node(def, &parsed) {
            Some(f) => f.range().start(),
            None => def.full_range(self.db, &parsed).range().start(),
        };
        (path, start)
    }

    pub(crate) fn qname(&self, def: Definition<'db>) -> String {
        let parsed = parsed_module(self.db, def.python_file(self.db)).load(self.db);
        qname::qualified_name(self.db, def, &parsed).unwrap_or_else(|| "<unknown>".into())
    }
}
