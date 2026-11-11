use tsrs_ast::SourceFileParseOptions;
use tsrs_core::tspath;

fn main() {
    let text = "function a() { b(); }\nfunction b() {}\n";
    let file_name = "/spike/a.ts";
    let opts = SourceFileParseOptions {
        file_name: file_name.to_string(),
        path: tspath::to_path(file_name, "/spike", true),
        external_module_indicator_options: Default::default(),
    };
    let kind = tsrs_core::get_script_kind_from_file_name(file_name);
    let file = tsrs_parser::parse_source_file(opts, text, kind);
    println!("statements: {}", file.statements().nodes().len());
}
