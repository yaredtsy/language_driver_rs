//! The example in the repo's README is this test. If the code changes the answer, this fails; if the README is edited away from the
//! code, this fails too.

use analyzer_core::edit::apply;
use python_syntax::{MarkerStyle, Placement, outline, plan_marker};

const STYLE: MarkerStyle<'static> = MarkerStyle {
    label: "ID",
    placement: Placement::Docstring,
};

#[test]
fn the_readme_example_is_true() {
    let text = "class Order:\n    def total(self):\n        return 1\n";
    let outline = outline(text, STYLE);
    let lines: Vec<String> = outline
        .defs
        .iter()
        .map(|d| format!("{:?} {}", d.kind, d.qualname))
        .collect();
    assert_eq!(lines, ["Class Order", "Method Order.total"]);

    let edit = plan_marker(text, &outline.defs[1], "3RGJ8M2K", STYLE).unwrap();
    let new = apply(text, &[edit]).unwrap();
    assert_eq!(
        new,
        "class Order:\n    def total(self):\n        \"\"\"ID: 3RGJ8M2K\"\"\"\n        return 1\n"
    );

    // the README shows exactly this
    let readme = std::fs::read_to_string("../../../../README.md").expect("the repo README");
    for shown in [
        "let text = \"class Order:\\n    def total(self):\\n        return 1\\n\";",
        "// Class Order",
        "// Method Order.total",
        "plan_marker(text, &outline.defs[1], \"3RGJ8M2K\", STYLE)",
        "//             \"\"\"ID: 3RGJ8M2K\"\"\"",
        "cd languages/python && cargo test -p python-syntax",
    ] {
        assert!(
            readme.contains(shown),
            "the README no longer shows {shown:?}"
        );
    }
}
