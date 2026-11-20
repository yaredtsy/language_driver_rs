//! `parse` never fails and never panics (BD1 §2.1): every fixture, cut at every 50th byte (moved back to a character boundary), and at every byte of the
//! short ones, still gives a tree whose spans are slices of the text it was given. The testkit's `cut_every_50_bytes` wants an outline; this runs the same
//! loop on `parse` directly.

mod common;

use bend_parser::parse;

fn cut_at(text: &str, mut at: usize) -> &str {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    &text[..at]
}

#[test]
fn every_fixture_cut_every_50_bytes() {
    for (name, text) in common::fixtures() {
        let mut cuts: Vec<usize> = (0..=text.len()).step_by(50).collect();
        cuts.push(text.len());
        for at in cuts {
            let cut = cut_at(&text, at);
            let tree = parse(cut);
            let bad = common::invariants(cut, &tree);
            assert!(bad.is_empty(), "{name} cut at {at}: {}", bad.join("\n"));
        }
    }
}

#[test]
fn every_fixture_cut_at_every_byte() {
    for (name, text) in common::fixtures() {
        if text.len() >= 400 {
            continue;
        }
        for at in 0..=text.len() {
            let cut = cut_at(&text, at);
            let tree = parse(cut);
            let bad = common::invariants(cut, &tree);
            assert!(bad.is_empty(), "{name} cut at {at}: {}", bad.join("\n"));
        }
    }
}

#[test]
fn odd_texts() {
    for text in [
        "",
        "\0",
        "@",
        "@unsafe",
        "def",
        "def f",
        "def f(",
        "def f(x",
        "def f(x:",
        "type",
        "type T<",
        "law",
        "law l:",
        "import",
        "\u{feff}def a(): 0",
        "#",
        "\"",
        "'",
        "def f(): \"",
        "def f(x: (",
        "def a(x: {a == }): 0",
        "\r",
        "\n\n\n",
        "def a() -> \u{1F30D}: 0",
        "def f(): match",
        "def f(): match x:",
        "def f(): match x:\n case",
        "def f(): match x:\n case a",
        "def f(): do",
        "def f(): do M",
        "def f(): do M<",
        "def f(): do M<>:",
        "def f(): do M<>:\n  x <-",
        "def f(): do M<>:\n  x :",
        "def f(): do M<>:\n  return",
        "def f(): x =>",
        "def f(): (x",
        "def f(): (x, ",
        "def f(): (x :",
        "def f(): [x :",
        "def f(): [x : T *",
        "def f(): [x : T ^",
        "def f(): \\{",
        "def f(): \\{ K:",
        "def f(): {x ==",
        "def f(): {x != y",
        "def f(): %",
        "def f(): %e@",
        "def f(): %e : P;",
        "def f(): @x",
        "def f(): @x:",
        "def f(): @x: A ->",
        "def f(): & x",
        "def f(): f(~",
        "def f(): f!",
        "def f(): f!(",
        "def f(): a[",
        "def f(): a[i] <-",
        "def f(): 1n+",
        "def f(): 1.5e",
        "def f(): 'a",
        "def f(): '\\u{",
        "def f(): \"\\u{12",
        "def f(): x =",
        "def f(): x : T =",
        "def f(): -",
        "def f(): +",
        "def f(): a <",
        "def f(): a < b <",
        "def f(): a<b,",
        "def f(): D<A",
        "def f(): ?",
        "def f(): Kind(",
        "law l:\n  for x:",
        "law l:\n  for x: A where",
        "type T is",
        "type T is Data:\n  K{",
        "type T is Data:\n  K{x:",
    ] {
        let tree = parse(text);
        let bad = common::invariants(text, &tree);
        assert!(bad.is_empty(), "{text:?}: {}", bad.join("\n"));
    }
}
