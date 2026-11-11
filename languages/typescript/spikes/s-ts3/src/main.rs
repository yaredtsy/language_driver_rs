use std::sync::Arc;
use std::time::Instant;

use tsrs_ast::{Node, SourceFile, get_source_file_of_node, is_call_expression, is_new_expression};
use tsrs_compiler::{Context, ProgramOptions};
use tsrs_core::{P, tspath};
use tsrs_checker::Checker;
use tsrs_ast::SymbolFlags;
use tsrs_tsoptions::ParseConfigHost;
use tsrs_vfs::{FS, bundled, cachedvfs, osvfs};

static QUIET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn quiet() -> bool {
    QUIET.load(std::sync::atomic::Ordering::Relaxed)
}

struct Host {
    fs: Arc<dyn FS>,
    cwd: String,
}

impl ParseConfigHost for Host {
    fn fs(&self) -> &dyn FS {
        &*self.fs
    }
    fn get_current_directory(&self) -> &str {
        &self.cwd
    }
}

fn rss_kb() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

fn line_of(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].matches('\n').count() + 1
}

fn start_offset(text: &str, node: P<Node>) -> usize {
    tsrs_scanner::skip_trivia(text, node.pos().expect_source("pos")) as usize
}

fn describe(node: P<Node>) -> String {
    let file = get_source_file_of_node(node).unwrap();
    let text = file.text();
    let target = match node.name() {
        Some(n) => n.end().expect_source("end") as usize,
        None => start_offset(text, node),
    };
    let name = node.name().map(|n| n.text().to_string()).unwrap_or_default();
    format!("{:?} {name} @ {}:{}", node.kind(), file.file_name(), line_of(text, target))
}

struct Stats {
    sites: usize,
    resolved: usize,
    unresolved: usize,
}

/// Which engine answer a call resolved by: the matched signature's declaration, else the callee symbol's declaration.
fn resolve(c: &mut Checker, call: P<Node>, callee: P<Node>) -> (&'static str, Option<P<Node>>) {
    let signature = c.get_resolved_signature_exported(call);
    if let Some(d) = signature.declaration() {
        return ("signature", Some(d));
    }
    let Some(mut symbol) = c.get_symbol_at_location_exported(callee) else {
        return ("none", None);
    };
    if symbol.flags().intersects(SymbolFlags::Alias) {
        symbol = c.get_aliased_symbol(symbol);
    }
    ("symbol", symbol.value_declaration())
}

fn visit(file: P<SourceFile>, checker: &mut Checker, node: P<Node>, st: &mut Stats) {
    if is_call_expression(node) || is_new_expression(node) {
        st.sites += 1;
        let text = file.text();
        let start = start_offset(text, node);
        let callee = node.expression().unwrap();
        let cs = start_offset(text, callee);
        let ce = callee.end().expect_source("end") as usize;
        let snippet: String = text[cs..ce].chars().take(40).collect();
        let snippet = snippet.replace('\n', " ");
        let line = line_of(text, start);
        match resolve(checker, node, callee) {
            (via, Some(d)) => {
                st.resolved += 1;
                if !quiet() {
                    println!("{line:>4}  {snippet:<40} -> [{via}] {}", describe(d));
                }
            }
            (_, None) => {
                st.unresolved += 1;
                if !quiet() {
                    println!("{line:>4}  {snippet:<40} -> UNRESOLVED");
                }
            }
        }
    }
    node.for_each_child(&mut |c| {
        visit(file, checker, c, st);
        false
    });
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let config_path = args.get(1).expect("usage: s-ts3 <tsconfig> <file>... [--quiet] [--free] [--loop=N]").clone();
    let quiet = args.iter().any(|a| a == "--quiet");
    let free = args.iter().any(|a| a == "--free");
    let rounds: usize =
        args.iter().find_map(|a| a.strip_prefix("--loop=")).and_then(|n| n.parse().ok()).unwrap_or(1);
    let files: Vec<&String> = args[2..].iter().filter(|a| !a.starts_with("--")).collect();
    QUIET.store(quiet || rounds > 1, std::sync::atomic::Ordering::Relaxed);
    for round in 1..=rounds {
        if rounds > 1 {
            println!("--- round {round}");
        }
        run_once(&config_path, &files, quiet || rounds > 1, free || rounds > 1, args.iter().any(|a| a == "--regions"));
    }
}

fn run_once(config_path: &str, files: &[&String], quiet: bool, free: bool, use_regions: bool) {
    let cwd = tspath::get_directory_path(config_path);
    // As `tsrs_lint::engine::run`: the whole request runs in one entered region; the program's loader threads get regions of their own.
    // Locals drop in reverse order: loaders, then the scope, then the region.
    let region = use_regions.then(|| tsrs_core::arena::Region::new_scratch(1 << 20));
    let _scope = region.as_ref().map(|r| r.enter());
    let loaders = Arc::new(tsrs_compiler::ProgramRegions::default());

    println!("rss at start: {} KB", rss_kb());
    let t = Instant::now();
    let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(cachedvfs::from(osvfs::fs())));
    let host = Host { fs: Arc::clone(&fs), cwd: cwd.clone() };
    let (config, diagnostics) =
        tsrs_tsoptions::get_parsed_command_line_of_config_file(config_path, None, None, &host, None);
    println!("config diagnostics: {}", diagnostics.len());
    let config = config.expect("no config");
    println!("config errors: {}, root files: {}", config.errors.len(), config.parsed_config.file_names.len());
    let chost = tsrs_compiler::new_compiler_host(&cwd, fs, &bundled::default_lib_path(), None, None, None);
    let mut options = ProgramOptions::new(P::new(config), chost);
    options.checker_regions = true;
    if use_regions {
        options.regions = Some(Arc::clone(&loaders));
    }
    let program = tsrs_compiler::new_program(options);
    let load = t.elapsed();
    println!("program loaded: {} source files in {load:?}; rss {} KB", program.get_source_files().len(), rss_kb());
    let diags = program.get_program_diagnostics();
    println!("program diagnostics: {}", diags.len());
    for d in diags.iter().take(3) {
        println!("  {}", d.message_text());
    }

    let mut checker = program.get_type_checker(&Context::default());
    let mut st = Stats { sites: 0, resolved: 0, unresolved: 0 };
    let t = Instant::now();
    for f in files {
        let abs = tspath::get_normalized_absolute_path(f, &cwd);
        let Some(file) = program.get_source_file(&abs) else {
            println!("NOT IN PROGRAM: {abs}");
            continue;
        };
        if !quiet {
            println!("=== {abs}");
        }
        let before = st.sites;
        let mut local = Stats { sites: 0, resolved: 0, unresolved: 0 };
        if quiet {
            // resolve without printing
            let saved = std::io::stdout();
            drop(saved);
        }
        visit(file, &mut checker, file.as_node(), &mut local);
        st.sites += local.sites;
        st.resolved += local.resolved;
        st.unresolved += local.unresolved;
        println!("  {} sites in {abs}", st.sites - before);
    }
    let resolve = t.elapsed();
    println!(
        "SUMMARY sites={} resolved={} unresolved={} resolve_time={resolve:?} rss {} KB",
        st.sites, st.resolved, st.unresolved, rss_kb()
    );
    let _ = Context::default();
    drop(checker);
    if free {
        let before = rss_kb();
        // SAFETY: nothing derived from the program is used after this point.
        unsafe { tsrs_compiler::free_unshared_program(program) };
        println!("freed program: rss {before} KB -> {} KB", rss_kb());
    }
}
