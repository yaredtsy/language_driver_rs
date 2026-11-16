use proc_macro::TokenStream;

/// `#[derive(Hello)] struct Name;` gives `impl Name { pub fn hello(&self) {} }`. No `syn`, no `quote`: the type's name is read from the text.
#[proc_macro_derive(Hello)]
pub fn hello(input: TokenStream) -> TokenStream {
    let text = input.to_string();
    let name = text
        .split_whitespace()
        .skip_while(|word| *word != "struct")
        .nth(1)
        .unwrap_or("Unknown")
        .trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_');
    format!("impl {name} {{ pub fn hello(&self) {{}} }}").parse().unwrap()
}
