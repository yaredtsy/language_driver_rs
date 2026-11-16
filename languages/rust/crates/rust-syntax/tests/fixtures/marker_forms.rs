// §3: a line is read after `///` and one optional space; then the label, optional blanks, `:` and a text, trimmed. An empty text is no marker.
/// ID: plain
fn plain() {}

///ID: no_space
fn no_space() {}

/// ID:no_space_after_colon
fn no_space_after_colon() {}

/// ID : blank_before_colon
fn blank_before_colon() {}

/// ID:     trimmed text with spaces   
fn trimmed() {}

///  ID: two_spaces_is_not_read
fn two_spaces() {}

/// ID:
fn empty_text() {}

/// ID:    
fn blank_text() {}

/// id: other_case
fn other_case() {}

/// IDX: other_label
fn other_label() {}

/// see ID: not_at_the_start
fn not_at_the_start() {}
