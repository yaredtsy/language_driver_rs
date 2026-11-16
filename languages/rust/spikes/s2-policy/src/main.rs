//! S-RS2: for every call-like site of the fixture, what does the engine say?
use std::path::PathBuf;

use ra_ap_hir::{
    AsAssocItem, AssocItemContainer, CallableKind, Function, HasSource, ModuleDef, PathResolution, Semantics,
    db::HirDatabase,
};
use ra_ap_ide_db::RootDatabase;
use ra_ap_project_model::RustLibSource;
use ra_ap_syntax::{AstNode, SyntaxNode, ast, ast::HasName};

fn fn_desc(db: &RootDatabase, f: Function) -> String {
    let name = f.name(db).as_str().to_owned();
    let container = match f.as_assoc_item(db).map(|a| a.container(db)) {
        None => "free".to_owned(),
        Some(AssocItemContainer::Trait(t)) => format!("trait {}", t.name(db).as_str()),
        Some(AssocItemContainer::Impl(i)) => match i.trait_(db) {
            Some(t) => format!("impl {} for _", t.name(db).as_str()),
            None => "inherent impl".to_owned(),
        },
    };
    let src = f.source(db).map(|s| if s.file_id.is_macro() { "macro-file" } else if s.value.syntax().text().to_string().contains("Default::default") { "real-file(derive-looking)" } else { "real-file" }).unwrap_or("no-source");
    let extra = if std::env::var_os("SRC").is_some() { f.source(db).map(|s| format!(" file_id_is_macro={} text={:?}", s.file_id.is_macro(), s.value.syntax().text().to_string().chars().take(60).collect::<String>())).unwrap_or_default() } else { String::new() };
    format!("fn {name} [{container}; {src}]{extra}")
}

fn def_desc(db: &RootDatabase, d: ModuleDef) -> String {
    match d {
        ModuleDef::Function(f) => fn_desc(db, f),
        ModuleDef::Adt(a) => format!("Adt {}", a.name(db).as_str()),
        ModuleDef::EnumVariant(v) => format!("Variant {}", v.name(db).as_str()),
        other => format!("{:?}", std::mem::discriminant(&other)),
    }
}

fn res_desc(db: &RootDatabase, r: &PathResolution<'_>) -> String {
    match r {
        PathResolution::Def(d) => def_desc(db, *d),
        PathResolution::Local(_) => "Local".into(),
        PathResolution::TypeParam(_) => "TypeParam".into(),
        PathResolution::SelfType(_) => "SelfType".into(),
        _ => "other".into(),
    }
}

fn kind_desc(db: &RootDatabase, k: CallableKind<'_>) -> String {
    match k {
        CallableKind::Function(f) => format!("Function({})", fn_desc(db, f)),
        CallableKind::TupleStruct(s) => format!("TupleStruct({})", s.name(db).as_str()),
        CallableKind::TupleEnumVariant(v) => format!("TupleEnumVariant({})", v.name(db).as_str()),
        CallableKind::Closure(_) => "Closure".into(),
        CallableKind::FnPtr => "FnPtr".into(),
        CallableKind::FnImpl(t) => format!("FnImpl({t:?})"),
    }
}

fn one_line(n: &SyntaxNode) -> String {
    let t = n.text().to_string().replace('\n', " ");
    if t.len() > 50 { format!("{}…", &t[..50]) } else { t }
}

struct Ctx<'a> {
    sema: &'a Semantics<'a, RootDatabase>,
    db: &'a RootDatabase,
    text: &'a str,
}

impl Ctx<'_> {
    fn orig(&self, n: &SyntaxNode) -> String {
        match self.sema.original_range_opt(n) {
            Some(r) => {
                let (s, e) = (usize::from(r.range.start()), usize::from(r.range.end()));
                format!("orig={:?}", self.text.get(s..e).map(|t| t.replace('\n', " ")).unwrap_or_else(|| "<other file>".into()))
            }
            None => "orig=None".into(),
        }
    }

    fn site(&self, n: &SyntaxNode, in_macro: bool) {
        let tag = if in_macro { "  [exp]" } else { "       " };
        if let Some(call) = ast::CallExpr::cast(n.clone()) {
            let Some(callee) = call.expr() else { return };
            let mut out = vec![];
            if let ast::Expr::PathExpr(pe) = &callee {
                if let Some(path) = pe.path() {
                    match self.sema.resolve_path_with_subst(&path) {
                        Some((r, sub)) => out.push(format!("path→{} subst={}", res_desc(self.db, &r), if sub.is_some() { "Some" } else { "None" })),
                        None => out.push("path→None".into()),
                    }
                }
            }
            match self.sema.type_of_expr(&callee).and_then(|t| t.original.as_callable(self.db)) {
                Some(c) => out.push(format!("callable={}", kind_desc(self.db, c.kind()))),
                None => out.push("callable=None".into()),
            }
            println!("{tag} CALL   `{}`  {}  {}", one_line(n), out.join(" | "), self.orig(n));
        } else if let Some(m) = ast::MethodCallExpr::cast(n.clone()) {
            let fb = match self.sema.resolve_method_call_fallback(&m) {
                Some((either, sub)) => {
                    let what = match either {
                        either::Either::Left(f) => fn_desc(self.db, f),
                        either::Either::Right(_) => "FIELD".into(),
                    };
                    format!("{what} subst={}", if sub.is_some() { "Some" } else { "None" })
                }
                None => "None".into(),
            };
            let plain = self.sema.resolve_method_call(&m).map(|f| fn_desc(self.db, f)).unwrap_or("None".into());
            println!("{tag} METHOD `{}`  fallback→{fb} | plain→{}  {}", one_line(n), if plain == "None" { "None" } else { "Some" }, self.orig(n));
        } else if let Some(r) = ast::RecordExpr::cast(n.clone()) {
            let by_path = r.path().and_then(|p| self.sema.resolve_path(&p)).map(|x| res_desc(self.db, &x)).unwrap_or("None".into());
            let by_variant = self.sema.resolve_variant(r.clone()).map(|v| format!("Variant {}", v.name(self.db).as_str())).unwrap_or("None".into());
            println!("{tag} RECORD `{}`  resolve_path→{by_path} | resolve_variant→{by_variant}  {}", one_line(n), self.orig(n));
        } else if let Some(p) = ast::PathExpr::cast(n.clone()) {
            // a path in value position that is not the callee of a call
            let is_callee = n.parent().and_then(ast::CallExpr::cast).is_some_and(|c| c.expr().is_some_and(|e| e.syntax() == n));
            let is_record_path = n.parent().is_some_and(|x| ast::RecordExpr::can_cast(x.kind()));
            if !is_callee && !is_record_path {
                if let Some(path) = p.path() {
                    let r = self.sema.resolve_path(&path).map(|x| res_desc(self.db, &x)).unwrap_or("None".into());
                    println!("{tag} PATH   `{}`  resolve_path→{r}  {}", one_line(n), self.orig(n));
                }
            }
        }
    }

    /// every site under `node`, descending into macro calls through their expansion
    fn walk(&self, node: &SyntaxNode, in_macro: bool) {
        for n in node.descendants() {
            if let Some(mc) = ast::MacroCall::cast(n.clone()) {
                let name = mc.path().map(|p| p.syntax().text().to_string()).unwrap_or_default();
                match self.sema.expand_macro_call(&mc) {
                    Some(exp) => {
                        println!("{}  MACRO `{name}!` expands ({} bytes)", if in_macro { "  [exp]" } else { "       " }, usize::from(exp.value.text().len()));
                        self.walk(&exp.value, true);
                    }
                    None => println!("       MACRO `{name}!` does NOT expand"),
                }
                continue;
            }
            self.site(&n, in_macro);
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("fixture dir"));
    let abs = std::fs::canonicalize(&root).unwrap();
    let (db, vfs) = s1_hir::load(&root, Some(RustLibSource::Discover)).expect("load");
    let lib = abs.join("src/lib.rs");
    let text = std::fs::read_to_string(&lib).unwrap();
    ra_ap_hir::attach_db(&db, || {
        let sema = Semantics::new(&db);
        let (file_id, _) = vfs.iter().find(|(_, p)| p.as_path().is_some_and(|p| AsRef::<std::path::Path>::as_ref(p) == lib)).expect("lib.rs in vfs");
        let efid = sema.attach_first_edition_opt(file_id).expect("edition");
        let file = sema.parse(efid);
        let cx = Ctx { sema: &sema, db: &db, text: &text };
        for f in file.syntax().descendants().filter_map(ast::Fn::cast) {
            let name = f.name().map(|n| n.text().to_string()).unwrap_or_default();
            let want = std::env::args().nth(2);
            let show = match &want { Some(w) => name.starts_with(w.as_str()), None => name.starts_with('c') && name.as_bytes().get(1).is_some_and(u8::is_ascii_digit) || name == "twice" || name == "run" };
            if !show { continue; }
            println!("--- {name}");
            if let Some(body) = f.body() { cx.walk(body.syntax(), false); }
        }
        let _: &dyn HirDatabase = &db;
    });
}
