use super::project::is_within;
use super::*;
use typescript_syntax::MarkerStyle;

fn three_files() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/three-files")
}

#[test]
fn the_program_has_one_checker() {
    let sem = Sem::open(&three_files(), Path::new("src/main.ts")).unwrap();
    assert_eq!(sem.program.options().checkers, Some(1));
}

#[test]
fn the_program_lists_its_files_in_the_same_order_every_time() {
    // Files are parsed on several threads, so the order the program ends with must not depend on which finished first.
    let names = || -> Vec<String> {
        let sem = Sem::open(&three_files(), Path::new("src/main.ts")).unwrap();
        sem.program
            .get_source_files()
            .iter()
            .map(|file| file.file_name().to_owned())
            .collect()
    };
    let first = names();
    assert!(
        first.len() > 3,
        "the bundled libraries are in the program too"
    );
    for _ in 0..30 {
        assert_eq!(names(), first);
    }
}

fn with_node_modules() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/with-node-modules")
}

/// (relative path or bundled, is project code, package) of every file in the program, sorted.
fn classified(sem: &Sem) -> Vec<(String, bool, Option<String>)> {
    let mut all: Vec<_> = sem
        .files()
        .map(|file| {
            (
                sem.relative_path(file)
                    .unwrap_or_else(|| "(outside root)".into()),
                sem.is_project_code(file),
                sem.package_of(file),
            )
        })
        .collect();
    all.sort();
    all
}

#[test]
fn project_code_is_told_apart_from_packages_declarations_and_libraries() {
    let sem = Sem::open(&with_node_modules(), Path::new("src/main.ts")).unwrap();
    let all = classified(&sem);
    let under_root: Vec<_> = all
        .iter()
        .filter(|(path, ..)| path != "(outside root)")
        .collect();
    let row = |path: &str, project: bool, package: Option<&str>| {
        (path.to_owned(), project, package.map(str::to_owned))
    };
    assert_eq!(
        under_root,
        [
            &row(
                "node_modules/@scope/other/index.d.ts",
                false,
                Some("@scope/other")
            ),
            &row("node_modules/@types/fake/index.d.ts", false, Some("fake")),
            &row("node_modules/pkg/index.d.ts", false, Some("pkg")),
            &row("src/main.ts", true, None),
            &row("src/types.d.ts", false, None),
        ]
    );
    // everything else is a bundled library: not project code, one package for all of them
    let libs: Vec<_> = all
        .iter()
        .filter(|(path, ..)| path == "(outside root)")
        .collect();
    assert!(libs.len() > 50, "{} libraries", libs.len());
    assert!(
        libs.iter()
            .all(|(_, project, package)| !project && package.as_deref() == Some("typescript:lib"))
    );
}

#[test]
fn a_repo_declaration_file_is_in_the_program_but_is_not_project_code() {
    let sem = Sem::open(&with_node_modules(), Path::new("src/main.ts")).unwrap();
    let types = sem
        .file("src/types.d.ts")
        .expect("main.ts imports it, so the program holds it");
    assert!(!sem.is_project_code(types));
    assert_eq!(sem.package_of(types), None);
}

#[test]
fn an_imported_json_file_is_held_by_the_program_and_is_not_project_code() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json-import");
    let sem = Sem::open(&root, Path::new("src/main.ts")).unwrap();
    let data = sem.file("src/data.json").expect("main.ts imports it");
    assert!(!sem.is_project_code(data));
    assert!(!sem.is_repo_declaration_file(data));
}

#[test]
fn a_file_is_found_by_its_relative_path() {
    let sem = Sem::open(&with_node_modules(), Path::new("src/main.ts")).unwrap();
    let file = sem.file("./src/../src/main.ts").unwrap();
    assert_eq!(sem.relative_path(file), Some("src/main.ts".into()));
    assert!(sem.file("src/gone.ts").is_none());
}

#[test]
fn a_windows_verbatim_path_is_spelled_the_way_the_engine_reads_it() {
    let engine = |path: &str| to_engine_path(Path::new(path));
    assert_eq!(engine(r"\\?\D:\repo\src\main.ts"), "D:/repo/src/main.ts");
    assert_eq!(
        engine(r"\\?\UNC\server\share\main.ts"),
        "//server/share/main.ts"
    );
    // already ordinary: unchanged but for the separator
    assert_eq!(engine(r"D:\repo\main.ts"), "D:/repo/main.ts");
    assert_eq!(engine("/repo/src/main.ts"), "/repo/src/main.ts");
}

#[test]
fn a_directory_is_within_the_root_whichever_way_each_is_spelled() {
    // the root is canonical, so on Windows verbatim; the directory of the climb is an engine path. Text on both sides, so this runs on every OS.
    assert!(is_within(
        Path::new(r"\\?\D:\repo"),
        Path::new("D:/repo/sub")
    ));
    assert!(is_within(Path::new(r"\\?\D:\repo"), Path::new("D:/repo")));
    assert!(is_within(Path::new(r"D:\repo"), Path::new("D:/repo/a/b")));
    assert!(is_within(Path::new("/repo"), Path::new("/repo/sub")));
    assert!(is_within(
        Path::new(r"\\?\UNC\server\share"),
        Path::new("//server/share/sub")
    ));
    // above the root, beside it, and a name that only starts with the root's
    assert!(!is_within(Path::new(r"\\?\D:\repo"), Path::new("D:/")));
    assert!(!is_within(Path::new(r"\\?\D:\repo"), Path::new("D:/other")));
    assert!(!is_within(
        Path::new(r"\\?\D:\repo"),
        Path::new("D:/repository")
    ));
    assert!(!is_within(Path::new("/repo"), Path::new("/repository")));
}

#[test]
fn a_package_is_named_from_the_last_node_modules_in_the_path() {
    let name = |path: &str| package_in_node_modules(path);
    assert_eq!(
        name("/r/node_modules/lodash/fp/index.js"),
        Some("lodash".into())
    );
    assert_eq!(
        name("/r/node_modules/@scope/pkg/a.js"),
        Some("@scope/pkg".into())
    );
    assert_eq!(
        name("/r/node_modules/@types/node/fs.d.ts"),
        Some("node".into())
    );
    assert_eq!(
        name("/r/node_modules/@types/babel__core/i.d.ts"),
        Some("@babel/core".into())
    );
    assert_eq!(
        name("/r/node_modules/.pnpm/react@18/node_modules/react/index.js"),
        Some("react".into())
    );
    assert_eq!(name("/r/src/node_modules_like/a.ts"), None);
    assert_eq!(name("/r/node_modules/"), None);
}

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: typescript_syntax::Placement::Docstring,
};

fn entries() -> Sem {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/entries");
    Sem::open(&root, Path::new("src/main.ts")).unwrap()
}

/// (qualname, start of the def in bytes) of the def `entry` finds.
fn found(sem: &Sem, qualname: &str, occurrence: u16) -> Option<(String, u32)> {
    let func = sem.entry("src/main.ts", qualname, occurrence, STYLE)?;
    let def = sem.def(func, STYLE);
    Some((def.qualname.clone(), def.range.start))
}

#[test]
fn entry_finds_the_nth_def_with_a_qualname_in_source_order() {
    let sem = entries();
    // overloads with an implementation are one def: `dup` has one
    assert!(found(&sem, "dup", 0).is_some());
    assert_eq!(found(&sem, "dup", 1), None);
    // signatures with no implementation are a def each, with one qualname
    let first = found(&sem, "ambient", 0).unwrap().1;
    let second = found(&sem, "ambient", 1).unwrap().1;
    assert!(first < second);
    assert_eq!(found(&sem, "ambient", 2), None);
    // a getter and a setter of one property
    let getter = found(&sem, "Box.size", 0).unwrap().1;
    let setter = found(&sem, "Box.size", 1).unwrap().1;
    assert!(getter < setter);
    assert_eq!(found(&sem, "Box.size", 2), None);
}

#[test]
fn entry_finds_every_kind_of_def_by_its_qualname() {
    let sem = entries();
    for qualname in ["Box", "Box.open", "arrow", "outer", "outer.<locals>.inner"] {
        assert_eq!(found(&sem, qualname, 0).unwrap().0, qualname);
    }
}

#[test]
fn entry_is_none_for_an_unknown_qualname_or_file() {
    let sem = entries();
    assert_eq!(found(&sem, "nope", 0), None);
    assert!(sem.entry("src/gone.ts", "dup", 0, STYLE).is_none());
    // `inner` alone is not its qualname
    assert_eq!(found(&sem, "inner", 0), None);
}

#[test]
fn a_def_of_a_file_that_is_not_code_cannot_be_an_entry() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json-import");
    let sem = Sem::open(&root, Path::new("src/main.ts")).unwrap();
    assert!(sem.entry("src/data.json", "items", 0, STYLE).is_none());
}

/// Every file of every fixture the TS1 outline suite has (it covers every kind of def), opened as a project of its own: the root is the
/// dialect's folder, which has no config above it, so each file is an inferred project.
fn outline_fixtures() -> Vec<(String, PathBuf, PathBuf)> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../typescript-syntax/tests/fixtures");
    let mut found = Vec::new();
    for dialect in ["ts", "tsx", "js", "jsx"] {
        let root = base.join(dialect);
        let mut names: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        for name in names {
            found.push((
                format!("{dialect}/{}", name.to_string_lossy()),
                root.clone(),
                PathBuf::from(name),
            ));
        }
    }
    found
}

#[test]
fn every_def_of_every_fixture_maps_to_exactly_one_program_node_and_back() {
    let mut checked_files = 0;
    let mut checked_defs = 0;
    let mut problems = Vec::new();
    for (label, root, file) in outline_fixtures() {
        let sem = Sem::open(&root, &file)
            .unwrap_or_else(|error| panic!("{label} does not open: {error}"));
        for held in sem.files() {
            // the files of the fixture folder (not the bundled libraries): a file the entry imports is checked too
            let Some(relative) = sem.relative_path(held) else {
                continue;
            };
            let Some(defs) = sem.file_defs(held, STYLE) else {
                continue;
            };
            checked_files += 1;
            let mut seen = HashSet::new();
            for (index, def) in defs.outline.defs.iter().enumerate() {
                checked_defs += 1;
                let Some(node) = defs.nodes[index] else {
                    problems.push(format!(
                        "{label} {relative}: `{}` has no node",
                        def.qualname
                    ));
                    continue;
                };
                if !seen.insert(node) {
                    problems.push(format!(
                        "{label} {relative}: `{}` shares its node",
                        def.qualname
                    ));
                }
                if defs.def_of.get(&node) != Some(&index) {
                    problems.push(format!(
                        "{label} {relative}: `{}` does not map back",
                        def.qualname
                    ));
                }
            }
            if defs.def_of.len() != defs.outline.defs.len() {
                problems.push(format!(
                    "{label} {relative}: {} nodes for {} defs",
                    defs.def_of.len(),
                    defs.outline.defs.len()
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
    // not a vacuous pass: every fixture file was checked, and they hold a good many defs
    assert_eq!(checked_files, outline_fixtures().len());
    assert!(checked_defs > 250, "{checked_defs} defs");
}

fn sites_project() -> Sem {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sites");
    Sem::open(&root, Path::new("src/sites.ts")).unwrap()
}

/// The callee text of every site of every def of `src/sites.ts` that has one, by qualname.
fn sites_by_def(sem: &Sem) -> std::collections::BTreeMap<String, Vec<String>> {
    let file = sem.file("src/sites.ts").unwrap();
    let text = file.node.text();
    let defs = sem.file_defs(file, STYLE).unwrap();
    let mut by_def = std::collections::BTreeMap::new();
    for (index, def) in defs.outline.defs.iter().enumerate() {
        let func = Func { file, def: index };
        let sites = sem
            .call_sites(func, STYLE)
            .expect("every def of this file has a node");
        let texts: Vec<String> = sites
            .iter()
            .map(|span| span.slice(text).unwrap().to_owned())
            .collect();
        if !texts.is_empty() {
            by_def.insert(def.qualname.clone(), texts);
        }
    }
    by_def
}

/// The text of every site of the def `qualname` of `src/accessors.ts` in the `site-kinds` fixture, in order.
fn accessor_sites_of(qualname: &str) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/site-kinds");
    let sem = Sem::open(&root, Path::new("src/accessors.ts")).unwrap();
    let func = sem.entry("src/accessors.ts", qualname, 0, STYLE).unwrap();
    let text = func.file.node.text();
    sem.call_sites(func, STYLE)
        .unwrap()
        .iter()
        .map(|span| span.slice(text).unwrap().to_owned())
        .collect()
}

#[test]
fn a_property_access_that_is_a_callee_is_one_site_and_not_also_an_accessor_site() {
    // two sites on one span would resolve to the same edge and be merged by the graph, so only the sites of the body can tell
    assert_eq!(accessor_sites_of("callee"), ["box.method", "box.handler"]);
    // each access of a chain is a site; the callee-position test looks through parentheses and assertions
    assert_eq!(accessor_sites_of("chain"), ["box.inner", "box.inner.value"]);
    // a field, a method read and not called, `delete` and a computed key are no sites
    assert!(accessor_sites_of("none").is_empty());
}

#[test]
fn a_body_s_plain_calls_are_its_sites_in_source_order() {
    let found = sites_by_def(&sites_project());
    let expect = |qualname: &str, sites: &[&str]| {
        assert_eq!(
            found
                .get(qualname)
                .map(|v| v.iter().map(String::as_str).collect::<Vec<_>>()),
            Some(sites.to_vec()),
            "{qualname}"
        );
    };
    // the callee expression is the site: not the arguments, not the type arguments
    expect(
        "plain",
        &["f", "a.b", "a?.b", "obj.x.y", "a[\"key\"]", "(g)"],
    );
    // by where the callee starts, then where it ends; an argument's call follows its callee's
    expect(
        "ordering",
        &["f", "g", "f", "f()", "g", "a.b", "a.b(f()).c", "f"],
    );
    expect(
        "wrappers",
        &["(f() as any).m", "f", "f", "f()!.n", "f", "g"],
    );
    // a parameter's default value and a destructuring default are part of the body
    expect("defaults", &["f", "g"]);
    // an anonymous function is part of the def around it
    expect("callbacks", &["xs.map", "g", "xs.forEach", "f", "f"]);
    // a named nested function and a const arrow are defs of their own
    expect("nested", &["f"]);
    expect("nested.<locals>.inner", &["g"]);
    expect("nested.<locals>.arrow", &["a.b"]);
    // `new Base()` is a site on the class expression, a tagged template on its tag; `import()` and `require()` are never, and `a.getterOnly` is one only when the
    // checker says it is an accessor (`a` is `any` here)
    expect("not_sites", &["Base", "f"]);
    // a class: `extends`, a property initializer, a static property, a static block and a decorator are its body; its methods are not
    // `@dec(1)` is the call of `dec` and, with the whole decorator expression, the call through its result
    expect("Widget", &["mixin", "f", "g", "a.init", "dec", "dec(1)"]);
    expect("Widget.method", &["f"]);
    expect("Widget.value", &["g"]);
    expect("Widget.constructor", &["super", "f"]);
    assert_eq!(found.get("Widget.decorated"), None);
    // an object-literal method is a def; a named function expression is a def; overloads have one body
    expect("table.load", &["f"]);
    // an arrow held by a property of a `const` object literal is a def too (D21): its call is its own, no longer nobody's
    expect("table.size", &["g"]);
    expect("expression_names", &["[1, 2].map"]);
    expect("expression_names.<locals>.named", &["g"]);
    expect("overloaded", &["f"]);
    expect("arrow_def", &["f", "g"]);
    // a namespace's body is its own (the code that runs when the namespace is first evaluated); the defs inside it are their own
    expect("Ns", &["f"]);
    expect("Ns.in_namespace", &["g"]);
    // a nested class is a def, but the calls in its computed member names belong to the function around it (as in tsrs)
    expect("holder", &["f"]);
    assert_eq!(found.get("holder.<locals>.Local"), None);
    // the type side of `satisfies` is not walked, the expression is
    expect("satisfying", &["(f() satisfies any).m", "f"]);
    // ambient code in a body is a declaration, not a call
    expect("ambient_in_body", &["g"]);
    // nothing else has a site: an interface method, an ambient `declare`, a leaf
    let listed = [
        "plain",
        "ordering",
        "wrappers",
        "defaults",
        "callbacks",
        "nested",
        "nested.<locals>.inner",
        "nested.<locals>.arrow",
        "not_sites",
        "Widget",
        "Widget.method",
        "Widget.value",
        "Widget.constructor",
        "table.load",
        "table.size",
        "expression_names",
        "expression_names.<locals>.named",
        "overloaded",
        "arrow_def",
        "Ns",
        "Ns.in_namespace",
        "holder",
        "satisfying",
        "ambient_in_body",
    ];
    let others: Vec<_> = found
        .keys()
        .filter(|q| !listed.contains(&q.as_str()))
        .collect();
    assert!(others.is_empty(), "{others:?}");
}

#[test]
fn a_files_defs_are_made_once_per_marker_style() {
    let sem = entries();
    let file = sem.file("src/main.ts").unwrap();
    let first = sem.file_defs(file, STYLE).unwrap();
    let again = sem.file_defs(file, STYLE).unwrap();
    assert!(Arc::ptr_eq(&first, &again));
    // a marker is read by its label, so another label is another outline
    let other = sem.file_defs(file, MarkerStyle::docstring("REF")).unwrap();
    assert!(!Arc::ptr_eq(&first, &other));
    assert!(Arc::ptr_eq(
        &other,
        &sem.file_defs(file, MarkerStyle::docstring("REF")).unwrap()
    ));
}

#[test]
fn a_def_is_read_in_place_not_copied() {
    let sem = entries();
    let func = sem.entry("src/main.ts", "Box.open", 0, STYLE).unwrap();
    let def = sem.def(func, STYLE);
    assert_eq!(def.qualname, "Box.open");
    // two reads share the one outline
    let again = sem.def(func, STYLE);
    assert!(std::ptr::eq(&*def, &*again));
}

/// What the engine says each site of `callers` in `tests/fixtures/resolve` calls, as `file qualname`, `external package name` or `unknown why`.
/// Written from the policy table (`docs/03-typescript/06-ts4-calls.md` §5) and the engine's own rule before the run.
fn resolved_sites(sem: &Sem, file: &str, qualname: &str) -> Vec<(String, Vec<String>)> {
    let source = sem.file(file).unwrap();
    let text = source.node.text();
    let func = sem.entry(file, qualname, 0, STYLE).unwrap();
    let spans = sem.call_sites(func, STYLE).unwrap();
    (0..spans.len())
        .map(|index| {
            let described = sem
                .callee(func, index, STYLE)
                .into_iter()
                .map(|resolved| match resolved {
                    Resolved::Def(target) => {
                        let info = sem.describe(target, STYLE);
                        // the nth of a def that shares its qualname (merged declarations) is shown, as `#n`, when it is not the first
                        match info.id {
                            analyzer_core::graph::StableDef::Path { nth, .. } if nth > 0 => {
                                format!("{} {}#{nth}", info.file, info.qname)
                            }
                            _ => format!("{} {}", info.file, info.qname),
                        }
                    }
                    Resolved::External { package, name } => {
                        format!("external {package} {name}")
                    }
                    Resolved::Unknown(why) => format!("unknown {}", why.counter()),
                })
                .collect();
            (spans[index].slice(text).unwrap().to_owned(), described)
        })
        .collect()
}

#[test]
fn sem_contract_each_site_resolves_as_the_policy_table_says() {
    let sem = Sem::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolve"),
        Path::new("src/main.ts"),
    )
    .unwrap();
    let found = resolved_sites(&sem, "src/main.ts", "callers");
    let one = |site: &str, target: &str| (site.to_owned(), vec![target.to_owned()]);
    let expected = vec![
        one("plain", "src/lib.ts plain"),
        one("counter.bump", "src/lib.ts Counter.bump"),
        one("Counter.make", "src/lib.ts Counter.make"),
        // an interface method: the signature, a leaf with no body, not the implementations
        one("writer.write", "src/lib.ts Writer.write"),
        // a union receiver: one target per member that declares the method (policy table). The engine's call hierarchy gives none and the
        // checker's signature gives only the first member, so the members are asked one by one
        (
            "either.write".to_owned(),
            vec![
                "src/lib.ts JsonWriter.write".to_owned(),
                "src/lib.ts CsvWriter.write".to_owned(),
            ],
        ),
        // overloads: the implementation, which is the one def
        one("overloaded", "src/lib.ts overloaded"),
        one("generic", "src/lib.ts generic"),
        one("fallback", "src/lib.ts fallback"),
        one("arrow", "src/lib.ts arrow"),
        // followed through `export *`, a namespace import and a renamed import to the real declaration
        one("reexported", "src/more.ts reexported"),
        one("lib.plain", "src/lib.ts plain"),
        one("renamed", "src/lib.ts plain"),
        // outside project code: a package, the repo's own `.d.ts` (named by its path), the bundled libraries
        one("helper", "external pkg helper"),
        one("declared", "external src/types.d.ts declared"),
        one("Math.max", "external typescript:lib Math.max"),
        one("[1].map", "external typescript:lib Array.map"),
        // the engine has no declaration to enter, and says why
        one("cb", "unknown function_typed_value"),
        one("anyValue.run", "unknown any_or_untyped"),
        one("anyValue", "unknown any_or_untyped"),
        one("(lib as any)[key]", "unknown computed_name"),
        // a computed name the checker narrows to a literal is resolved
        one("counter[\"bump\"]", "src/lib.ts Counter.bump"),
        // a union through an element access with a literal name: the members are asked the same way
        (
            "either[\"write\"]".to_owned(),
            vec![
                "src/lib.ts JsonWriter.write".to_owned(),
                "src/lib.ts CsvWriter.write".to_owned(),
            ],
        ),
        // members that hold an arrow (a def named like a method) are targets too, as they are on a plain receiver
        (
            "arrows.write".to_owned(),
            vec![
                "src/lib.ts ArrowA.write".to_owned(),
                "src/lib.ts ArrowB.write".to_owned(),
            ],
        ),
        // `undefined` in the union has no property and adds nothing
        (
            "maybe?.write".to_owned(),
            vec![
                "src/lib.ts JsonWriter.write".to_owned(),
                "src/lib.ts CsvWriter.write".to_owned(),
            ],
        ),
        // an inherited method is where it is declared
        one("child.bump", "src/lib.ts Counter.bump"),
        // a method of an object literal is a def (D9)
        one("api.load", "src/lib.ts api.load"),
        // merged declarations: one target per declaration, told apart by their nth
        (
            "reader.read".to_owned(),
            vec![
                "src/lib.ts Reader.read".to_owned(),
                "src/lib.ts Reader.read#1".to_owned(),
            ],
        ),
    ];
    assert_eq!(found, expected);
}

#[test]
fn sem_contract_a_function_typed_value_is_one_even_when_optional_or_in_a_union() {
    let sem = Sem::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolve"),
        Path::new("src/main.ts"),
    )
    .unwrap();
    let unknown = |site: &str| {
        (
            site.to_owned(),
            vec!["unknown function_typed_value".to_owned()],
        )
    };
    // `cb?.()` where `cb` may be undefined, and `result.mutate()` where `result` is a union whose every member holds a function (what a
    // react-query mutation looks like): both are calls through a function-typed value (D16), not a union problem and not "no declaration"
    assert_eq!(
        resolved_sites(&sem, "src/main.ts", "optional_callbacks"),
        [unknown("cb"), unknown("result.mutate")]
    );
}

#[test]
fn sem_contract_function_typed_value_is_a_value_that_is_declared_as_one() {
    let sem = Sem::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolve"),
        Path::new("src/main.ts"),
    )
    .unwrap();
    let one = |site: &str, target: &str| (site.to_owned(), vec![target.to_owned()]);
    // a `let` that holds an arrow (only `const` names a def) and a destructured binding are value holders: D16
    assert_eq!(
        resolved_sites(&sem, "src/main.ts", "value_holders"),
        [
            one("later", "unknown function_typed_value"),
            one("onSave", "unknown function_typed_value")
        ]
    );
    // a callee that is not a name has no symbol, so it is not a value holder and nobody declared it: the engine has nothing to say, and the
    // count of `function_typed_value` does not swallow it
    assert_eq!(
        resolved_sites(&sem, "src/main.ts", "no_symbol"),
        [
            one("makeHandler", "src/lib.ts makeHandler"),
            one("makeHandler()", "unknown no_declaration"),
            one("(flag ? plain : second)", "unknown no_declaration"),
        ]
    );
}

#[test]
fn sem_contract_an_external_that_is_declared_as_a_value_is_an_external() {
    let sem = Sem::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolve"),
        Path::new("src/main.ts"),
    )
    .unwrap();
    let one = |site: &str, target: &str| (site.to_owned(), vec![target.to_owned()]);
    // `export declare const useThing: () => number` and a property of function type in a package are named calls into the package, as a function
    // declaration would be; they are not D16's function-typed values, which are the ones declared in the project
    assert_eq!(
        resolved_sites(&sem, "src/main.ts", "external_value_holders"),
        [
            one("useThing", "external pkg useThing"),
            one("toolbox.run", "external pkg toolbox.run")
        ]
    );
}

#[test]
fn sem_contract_this_dot_method_is_the_method_of_the_class() {
    let sem = Sem::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolve"),
        Path::new("src/main.ts"),
    )
    .unwrap();
    assert_eq!(
        resolved_sites(&sem, "src/lib.ts", "Greeter.run"),
        [(
            "this.helper".to_owned(),
            vec!["src/lib.ts Greeter.helper".to_owned()]
        )]
    );
}

#[test]
fn describe_names_a_def_by_its_marker_or_by_where_it_is() {
    use analyzer_core::graph::StableDef;
    let resolve = Sem::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolve"),
        Path::new("src/main.ts"),
    )
    .unwrap();
    // a def with a marker is named by it: it survives a rename or a move
    let plain = resolve.entry("src/lib.ts", "plain", 0, STYLE).unwrap();
    let info = resolve.describe(plain, STYLE);
    assert_eq!(info.id, StableDef::Marked("PLAIN-0001".into()));
    assert_eq!(info.qname, "plain");
    assert_eq!(info.file, "src/lib.ts");
    assert_eq!(info.range, resolve.def(plain, STYLE).range);
    // one without is named by its file, qualname and which of the same-named defs it is
    let bump = resolve
        .entry("src/lib.ts", "Counter.bump", 0, STYLE)
        .unwrap();
    assert_eq!(
        resolve.describe(bump, STYLE).id,
        StableDef::Path {
            module: "src/lib.ts".into(),
            qname: "Counter.bump".into(),
            nth: 0
        }
    );
    // a getter and its setter have one qualname: the second is nth 1
    let entries = entries();
    let setter = entries.entry("src/main.ts", "Box.size", 1, STYLE).unwrap();
    assert_eq!(
        entries.describe(setter, STYLE).id,
        StableDef::Path {
            module: "src/main.ts".into(),
            qname: "Box.size".into(),
            nth: 1
        }
    );
    // and a class is described like a function
    let class = entries.entry("src/main.ts", "Box", 0, STYLE).unwrap();
    assert_eq!(
        entries.describe(class, STYLE).kind,
        analyzer_core::outline::DefKind::Class
    );
}

#[test]
fn sem_contract_in_a_script_the_project_function_with_a_body_wins_over_the_libs_global() {
    let sem = Sem::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/script-global"),
        Path::new("main.ts"),
    )
    .unwrap();
    let one = |site: &str, target: &str| (site.to_owned(), vec![target.to_owned()]);
    // `function fetch() {}` in a script is the same symbol as the DOM's `fetch`; the checker lists both declarations, and a call to the project's
    // function is not a call into the browser. Our choice among the engine's declarations (ENGINE.md): one with a body in project code wins when the
    // others are in a bundled library or a .d.ts
    assert_eq!(
        resolved_sites(&sem, "main.ts", "caller"),
        [
            one("fetch", "main.ts fetch"),
            one("open", "main.ts open"),
            // the project's `declare function alert` has no body, so nothing is preferred: both targets stay, as the engine says
            (
                "alert".to_owned(),
                vec![
                    "main.ts alert".to_owned(),
                    "external typescript:lib alert".to_owned()
                ]
            ),
        ]
    );
}
