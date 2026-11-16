//! S-RS4: where do the `///` lines of every kind of def live in the tree?
use ra_ap_syntax::{
    AstNode, Edition, NodeOrToken, SourceFile, SyntaxNode,
    ast::{self, HasAttrs, HasName},
};

fn short(s: &str) -> String {
    let s = s.replace('\n', "\\n");
    if s.len() > 28 { format!("{}…", &s[..28]) } else { s }
}

/// The direct children of `node`, in order, one token or node per entry.
fn children(node: &SyntaxNode) -> String {
    node.children_with_tokens()
        .take_while(|c| !matches!(c.kind(), ra_ap_syntax::SyntaxKind::NAME))
        .map(|c| match c {
            NodeOrToken::Node(n) => format!("{:?}[{}]", n.kind(), short(&n.text().to_string())),
            NodeOrToken::Token(t) => format!("{:?}[{}]", t.kind(), short(t.text())),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn show(case: &str, src: &str) {
    println!("--- {case}");
    let file = SourceFile::parse(src, Edition::Edition2024);
    assert!(file.errors().is_empty(), "{case}: {:?}", file.errors());
    let file = file.tree();
    for item in file.syntax().descendants().filter_map(ast::Item::cast) {
        let n = item.syntax();
        let name = ast::AnyHasName::cast(n.clone()).and_then(|h| h.name()).map(|n| n.text().to_string());
        let r = n.text_range();
        let has_doc = ast::AnyHasAttrs::cast(n.clone()).unwrap();
        let docs: Vec<String> = has_doc.doc_comments().map(|d| short(&d.syntax().text().to_string())).collect();
        let attrs: Vec<String> = ast::AnyHasAttrs::cast(n.clone()).unwrap().attrs().map(|a| short(&a.syntax().text().to_string())).collect();
        println!(
            "{:?} {:?} node={}..{} first-line={:?}\n    children: {}\n    doc_comments(): {:?}  attrs(): {:?}",
            n.kind(), name, u32::from(r.start()), u32::from(r.end()),
            short(&src[usize::from(r.start())..].lines().next().unwrap_or("")),
            children(n), docs, attrs
        );
    }
}

fn main() {
    // every kind of def, one `///` above
    let kinds: &[(&str, &str)] = &[
        ("fn", "/// d\nfn f() {}\n"),
        ("method in inherent impl", "struct S;\nimpl S {\n    /// d\n    fn m(&self) {}\n}\n"),
        ("method in trait impl", "trait T { fn m(&self); }\nstruct S;\nimpl T for S {\n    /// d\n    fn m(&self) {}\n}\n"),
        ("trait method, no body", "trait T {\n    /// d\n    fn m(&self);\n}\n"),
        ("trait method, body", "trait T {\n    /// d\n    fn m(&self) {}\n}\n"),
        ("struct", "/// d\nstruct S;\n"),
        ("enum", "/// d\nenum E { A }\n"),
        ("union", "/// d\nunion U { a: u32 }\n"),
        ("trait", "/// d\ntrait T {}\n"),
        ("inline mod", "/// d\nmod m {}\n"),
        ("type alias", "/// d\ntype A = u32;\n"),
        ("const", "/// d\nconst C: u32 = 1;\n"),
        ("static", "/// d\nstatic S: u32 = 1;\n"),
        ("macro_rules", "/// d\nmacro_rules! m { () => {} }\n"),
        ("macro_rules with #[macro_export]", "/// d\n#[macro_export]\nmacro_rules! m { () => {} }\n"),
        ("fn in fn", "fn outer() {\n    /// d\n    fn inner() {}\n}\n"),
        ("foreign fn", "unsafe extern \"C\" {\n    /// d\n    fn c();\n}\n"),
        ("impl block itself", "struct S;\n/// d\nimpl S {}\n"),
        ("enum variant", "enum E {\n    /// d\n    A,\n}\n"),
        ("record field", "struct S {\n    /// d\n    a: u32,\n}\n"),
    ];
    for (c, s) in kinds { show(c, s); }

    // the questions of the table
    let q: &[(&str, &str)] = &[
        ("Q1 doc + attrs interleaved", "/// a\n#[inline]\n/// b\n#[must_use]\nfn f() {}\n"),
        ("Q1 attr first, then doc", "#[inline]\n/// a\nfn f() {}\n"),
        ("Q2 blank between doc block and item", "/// a\n\nfn f() {}\n"),
        ("Q2 blank between two /// lines", "/// a\n\n/// b\nfn f() {}\n"),
        ("Q2 two blank lines between doc block and item", "/// a\n\n\nfn f() {}\n"),
        ("Q2 blank, then attr, then item", "/// a\n\n#[inline]\nfn f() {}\n"),
        ("Q3 plain // directly above", "// plain\nfn f() {}\n"),
        ("Q3 plain // then blank then item", "// plain\n\nfn f() {}\n"),
        ("Q3 plain // above /// ", "// plain\n/// d\nfn f() {}\n"),
        ("Q3 /// above plain //", "/// d\n// plain\nfn f() {}\n"),
        ("Q3 /// blank plain // item", "/// d\n\n// plain\nfn f() {}\n"),
        ("Q3 plain // blank /// item", "// plain\n\n/// d\nfn f() {}\n"),
        ("Q4 block doc /** */", "/** d */\nfn f() {}\n"),
        ("Q4 block doc multi-line", "/**\n * d\n * ID: x\n */\nfn f() {}\n"),
        ("Q4 #[doc = \"..\"]", "#[doc = \"d\"]\nfn f() {}\n"),
        ("Q4 //! inside a fn body", "fn f() {\n    //! inner\n    let _x = 1;\n}\n"),
        ("Q4 //! inside a mod", "mod m {\n    //! inner\n    fn g() {}\n}\n"),
        ("Q4 //// four slashes", "//// not doc\nfn f() {}\n"),
        ("Q4 /*** three stars", "/*** not doc */\nfn f() {}\n"),
        ("file header, blank, doc, item", "// header\n\n/// d\nfn f() {}\n"),
        ("//! at file top then item", "//! crate doc\n\n/// d\nfn f() {}\n"),
        ("CRLF line endings", "/// a\r\n/// b\r\nfn f() {}\r\n"),
        ("CRLF blank line", "/// a\r\n\r\nfn f() {}\r\n"),
        ("trailing comment after item, same line", "fn f() {} // c\n/// d\nfn g() {}\n"),
    ];
    for (c, s) in q { show(c, s); }

    // follow-ups
    show("CRLF plain // blank item", "// plain\r\n\r\nfn f() {}\r\n");
    show("CRLF /// blank item", "/// a\r\n\r\nfn f() {}\r\n");
    println!("--- tree of //! in a fn body");
    let f = SourceFile::parse("fn f() {\n    //! inner\n    let _x = 1;\n}\n", Edition::Edition2024).tree();
    println!("{:#?}", f.syntax());
    println!("--- tree of a doc line");
    let f = SourceFile::parse("/// ID: x\nfn f() {}\n", Edition::Edition2024).tree();
    println!("{:#?}", f.syntax());

    // corrections of 2026-11-16
    let ctx = |case: &str, src: &str| {
        println!("=== {case}");
        let file = SourceFile::parse(src, Edition::Edition2024).tree();
        println!("  SOURCE_FILE children: {}", children_all(file.syntax()));
        for f in file.syntax().descendants().filter_map(ast::Fn::cast) {
            let n = f.syntax();
            let rs = range_start(n);
            let r = n.text_range();
            println!(
                "  fn {:?}: node starts {} ({:?}), RS1 range start {} ({:?}); node children: {}",
                f.name().map(|n| n.text().to_string()),
                u32::from(r.start()), short(&src[usize::from(r.start())..]),
                rs, short(&src[rs as usize..]),
                children(n)
            );
            println!("  doc_comments(): {:?}", f.doc_comments().map(|d| short(d.text_with_markers())).collect::<Vec<_>>());
        }
    };
    ctx("C1 /// a, // plain, blank, /// b, item (LF)", "/// a\n// plain\n\n/// b\nfn f() {}\n");
    ctx("C1 same, CRLF", "/// a\r\n// plain\r\n\r\n/// b\r\nfn f() {}\r\n");
    ctx("C1 control: /// a, blank, // plain, /// b (all attached?)", "/// a\n\n// plain\n/// b\nfn f() {}\n");
    ctx("C1 control: /// a, blank, /// b", "/// a\n\n/// b\nfn f() {}\n");
    ctx("C1 /// a, // plain, blank, item", "/// a\n// plain\n\nfn f() {}\n");
    ctx("C1 /// a, //! inner-looking? (// plain, blank, #[attr], item)", "/// a\n// plain\n\n#[inline]\nfn f() {}\n");
    ctx("C2 header directly above the first item (LF)", "// ID: x\nfn f() {}\n");
    ctx("C2 header, blank, item (LF)", "// ID: x\n\nfn f() {}\n");
    ctx("C2 header, blank, item (CRLF)", "// ID: x\r\n\r\nfn f() {}\r\n");
    ctx("C2 header directly above the first item (CRLF)", "// ID: x\r\nfn f() {}\r\n");
    ctx("C2 header, blank, /// doc, item (CRLF)", "// ID: x\r\n\r\n/// d\r\nfn f() {}\r\n");
    ctx("C2 header, blank, #[attr], item (CRLF)", "// ID: x\r\n\r\n#[inline]\r\nfn f() {}\r\n");
    ctx("C2 shebang, header, blank, item", "#!/usr/bin/env rust-script\n// ID: x\n\nfn f() {}\n");
    ctx("C2 header, blank, //! inner doc, blank, item", "// ID: x\n\n//! crate\n\nfn f() {}\n");
}

fn children_all(node: &SyntaxNode) -> String {
    node.children_with_tokens()
        .map(|c| match c {
            NodeOrToken::Node(n) => format!("{:?}@{}[{}]", n.kind(), u32::from(n.text_range().start()), short(&n.text().to_string())),
            NodeOrToken::Token(t) => format!("{:?}@{}[{}]", t.kind(), u32::from(t.text_range().start()), short(t.text())),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// RS1's rule: the first `DOC_COMMENT` or `ATTR` child, else the first child that is not a comment or whitespace.
fn range_start(node: &SyntaxNode) -> u32 {
    use ra_ap_syntax::SyntaxKind::*;
    for c in node.children_with_tokens() {
        let k = c.kind();
        if matches!(k, DOC_COMMENT | ATTR) || !matches!(k, COMMENT | WHITESPACE) {
            return u32::from(c.text_range().start());
        }
    }
    u32::from(node.text_range().start())
}
