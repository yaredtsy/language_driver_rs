use std::time::Instant;

use tsrs_ast::{Node, SourceFileParseOptions, is_function_declaration};
use tsrs_core::{P, tspath};

fn rss_kb() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

fn source(lines: usize) -> String {
    let mut s = String::new();
    for i in 0..lines / 4 {
        s.push_str(&format!("/** doc {i} */\nexport function f{i}(a: number): number {{\n  return g{i}(a) + 1;\n}}\n"));
    }
    s
}

fn collect(node: P<Node>, out: &mut Vec<String>) {
    if is_function_declaration(node) {
        out.push(node.name().map(|n| n.text().to_string()).unwrap_or_default());
    }
    node.for_each_child(&mut |c| {
        collect(c, out);
        false
    });
}

fn parse_and_walk(text: &str, name: &str) -> Vec<String> {
    let opts = SourceFileParseOptions {
        file_name: name.to_string(),
        path: tspath::to_path(name, "/spike", true),
        external_module_indicator_options: Default::default(),
    };
    let kind = tsrs_core::get_script_kind_from_file_name(name);
    let file = tsrs_parser::parse_source_file(opts, text, kind);
    let mut names = Vec::new();
    collect(file.as_node(), &mut names);
    names
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "region".into());
    let rounds: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(10_000);
    let lines: usize = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(2_000);
    let text = source(lines);
    println!("mode={mode} rounds={rounds} lines={lines} bytes={}", text.len());
    println!("round 0: rss {} KB", rss_kb());
    let start = Instant::now();
    let mut count = 0;
    for round in 1..=rounds {
        let names = match mode.as_str() {
            "region" => {
                let region = tsrs_core::arena::Region::new_scratch(64 * 1024);
                let _scope = region.enter();
                parse_and_walk(&text, "/spike/a.ts")
            }
            _ => parse_and_walk(&text, "/spike/a.ts"),
        };
        count = names.len();
        if round % 1000 == 0 {
            println!("round {round}: rss {} KB, avg {:?}/parse", rss_kb(), start.elapsed() / round as u32);
        }
    }
    println!("functions per parse: {count}");
}
