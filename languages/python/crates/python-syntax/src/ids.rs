//! Reading a marker out of a docstring. The label (`ID`) is the caller's, from `MarkerStyle`.

/// The value of `<label>: <value>` in `doc`, the last one if there are several. The reader is deliberately loose: it looks
/// at every `:`, takes the word before it and the word after it, and ignores a pair where either word holds another `:`.
/// So prose that mentions the label (`The text says ID: nothing-real, ...`) counts as a marker. That is what pylspt did and
/// what users' files were marked with: do not tighten it without a migration.
pub(crate) fn read(doc: &str, label: &str) -> Option<String> {
    let mut found = None;
    for (i, _) in doc.char_indices().filter(|(_, c)| *c == ':') {
        let key = doc[..i].split_whitespace().next_back();
        let value = doc[i + 1..].split_whitespace().next();
        if let (Some(k), Some(v)) = (key, value)
            && k == label
            && !k.contains(':')
            && !v.contains(':')
        {
            found = Some(v.to_owned());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(doc: &str) -> Option<String> {
        read(doc, "ID")
    }

    #[test]
    fn reads_the_value_after_the_label() {
        assert_eq!(id("ID: abc").as_deref(), Some("abc"));
        assert_eq!(id("ID:abc").as_deref(), Some("abc"));
        assert_eq!(id("Summary.\n\n    ID: abc\n    ").as_deref(), Some("abc"));
    }

    #[test]
    fn the_last_one_wins() {
        assert_eq!(id("ID: a\nID: b").as_deref(), Some("b"));
    }

    #[test]
    fn prose_that_mentions_the_label_counts() {
        // pylspt's measured behaviour, and what users' files were marked with
        let doc = "The text says ID: nothing-real, and nothing real below.";
        assert_eq!(id(doc).as_deref(), Some("nothing-real,"));
    }

    #[test]
    fn no_value_or_a_value_with_a_colon_is_no_marker() {
        assert_eq!(id("ID:"), None);
        assert_eq!(id("ID:   "), None);
        assert_eq!(id("ID: a:b"), None);
        assert_eq!(id("no marker here"), None);
    }

    #[test]
    fn the_label_is_the_callers_and_must_be_the_whole_word() {
        assert_eq!(read("KeyID: x", "ID"), None);
        assert_eq!(read("KeyID: x", "KeyID").as_deref(), Some("x"));
        assert_eq!(read("ID: x", "Key"), None);
    }
}
