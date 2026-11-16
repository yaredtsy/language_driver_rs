//! S-RS2 oracle: are `Analysis::outgoing_calls` ranges a subset of ours, and comparable?
use std::{collections::BTreeSet, path::PathBuf};

use ra_ap_hir::Semantics;
use ra_ap_ide::{AnalysisHost, CallHierarchyConfig, FilePosition, RaFixtureConfig};
use ra_ap_ide_db::RootDatabase;
use ra_ap_syntax::{AstNode, SyntaxNode, ast, ast::HasName};

type Range = (usize, usize);

/// our sites: callee expression of a call, name of a method call; macro expansions entered; only what maps to a written range
fn ours(sema: &Semantics<'_, RootDatabase>, node: &SyntaxNode, out: &mut BTreeSet<Range>, dropped: &mut usize) {
    for n in node.descendants() {
        if let Some(mc) = ast::MacroCall::cast(n.clone()) {
            if let Some(exp) = sema.expand_macro_call(&mc) {
                ours(sema, &exp.value, out, dropped);
            }
            continue;
        }
        let target = if let Some(c) = ast::CallExpr::cast(n.clone()) {
            c.expr().map(|e| e.syntax().clone())
        } else if let Some(m) = ast::MethodCallExpr::cast(n.clone()) {
            m.name_ref().map(|x| x.syntax().clone())
        } else {
            None
        };
        if let Some(t) = target {
            match sema.original_range_opt(&t) {
                Some(r) => {
                    out.insert((usize::from(r.range.start()), usize::from(r.range.end())));
                }
                None => *dropped += 1,
            }
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).unwrap());
    let abs = std::fs::canonicalize(&root).unwrap();
    let (db, vfs) = s1_hir::load(&root, Some(ra_ap_project_model::RustLibSource::Discover)).unwrap();
    let lib = abs.join("src/lib.rs");
    let text = std::fs::read_to_string(&lib).unwrap();
    let (file_id, _) = vfs.iter().find(|(_, p)| p.as_path().is_some_and(|p| AsRef::<std::path::Path>::as_ref(p) == lib)).unwrap();

    // ours first, on the same database
    let mut ours_by_fn: Vec<(String, usize, BTreeSet<Range>, usize)> = vec![];
    ra_ap_hir::attach_db(&db, || {
        let sema = Semantics::new(&db);
        let file = sema.parse(sema.attach_first_edition_opt(file_id).unwrap());
        for f in file.syntax().descendants().filter_map(ast::Fn::cast) {
            let name = f.name().unwrap().text().to_string();
            if !["c01_", "c04b", "c06a", "c07a", "c09a", "c10a", "c11b", "c12_"].iter().any(|p| name.starts_with(p)) { continue; }
            let off = usize::from(f.name().unwrap().syntax().text_range().start());
            let mut set = BTreeSet::new();
            let mut dropped = 0;
            if let Some(b) = f.body() { ours(&sema, b.syntax(), &mut set, &mut dropped); }
            ours_by_fn.push((name, off, set, dropped));
        }
    });

    let host = AnalysisHost::with_database(db);
    let analysis = host.analysis();
    let cfg = CallHierarchyConfig { exclude_tests: false, ra_fixture: RaFixtureConfig::default() };
    for (name, off, ours, dropped) in ours_by_fn {
        let items = analysis.outgoing_calls(&cfg, FilePosition { file_id, offset: (off as u32).into() }).unwrap().unwrap_or_default();
        let mut oracle: BTreeSet<Range> = BTreeSet::new();
        println!("--- {name}");
        for it in &items {
            for r in &it.ranges {
                let rg = (usize::from(r.range.start()), usize::from(r.range.end()));
                oracle.insert(rg);
                println!("  oracle  {:<14} -> {:?} ({})", it.target.name, &text[rg.0..rg.1], if ours.contains(&rg) { "also ours" } else { "NOT IN OURS" });
            }
        }
        let only_ours: Vec<_> = ours.difference(&oracle).map(|r| text[r.0..r.1].to_owned()).collect();
        println!("  ours: {} ranges, oracle: {}; only ours: {:?}; ours dropped (no original range, macro body): {}", ours.len(), oracle.len(), only_ours, dropped);
        assert!(oracle.is_subset(&ours), "oracle has sites we lack");
    }
}
