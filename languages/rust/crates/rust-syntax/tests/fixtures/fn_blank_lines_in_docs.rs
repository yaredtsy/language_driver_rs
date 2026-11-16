/// a

fn blank_after_doc() {}

/// a
// plain

/// b
fn plain_between_docs() {}

/// a
// plain

fn doc_plain_blank_item() {}

//! an inner doc comment ends the run above

/// d
fn after_inner_doc() {}
