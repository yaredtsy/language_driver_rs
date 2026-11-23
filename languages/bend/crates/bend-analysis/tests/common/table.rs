//! The grammar of the last cell of a table row ("Where a row is checked" in `languages/bend/docs/policy.md`), shared by the tests that read a table of that file:
//! `policy.rs` (the layer-1 rows, header `| Row |`) and `context.rs` (the precise rows, header `| Precise row |`).
//!
//! The grammar, read from the doc: **groups** separated by `; `; a group is a fixture folder (also the root the book is opened at), one file of it (it ends in
//! `.bend`), then the entries (qualnames; `name#1` is a second def of one name); a group that begins with a file keeps the folder of the group before it. One reading
//! beyond the words: a group of **entries only** is more entries of the file of the group before it, since nothing else in the grammar could give it a meaning.

use std::fs;
use std::path::Path;

/// What the last cell says.
pub(crate) enum Cell {
    /// No word, or a shape the grammar does not have: the sentence says which.
    Broken(String),
    Groups(Vec<Group>),
}

/// A fixture folder, one file of it, and the entries built from that file.
pub(crate) struct Group {
    pub(crate) folder: String,
    pub(crate) file: String,
    pub(crate) entries: Vec<String>,
}

pub(crate) struct Row {
    pub(crate) id: String,
    pub(crate) cell: Cell,
}

/// The text between pairs of backticks, in order.
fn backticked(text: &str) -> Vec<String> {
    text.split('`')
        .enumerate()
        .filter(|(n, _)| n % 2 == 1)
        .map(|(_, word)| word.to_owned())
        .collect()
}

/// The rows of the table whose header line begins with `header` (`| Row |`, `| Precise row |`): to the first line that does not begin with `|`. The checked-where cell is the last.
pub(crate) fn rows(table: &str, header: &str) -> Vec<Row> {
    table
        .lines()
        .skip_while(|line| !line.starts_with(header))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            let id = backticked(line).into_iter().next().unwrap_or_default();
            let last = line
                .trim_end()
                .trim_end_matches('|')
                .rsplit_once(" | ")
                .map_or("", |(_, cell)| cell);
            Row {
                id,
                cell: groups(last),
            }
        })
        .collect()
}

fn is_file(word: &str) -> bool {
    word.ends_with(".bend")
}

fn groups(cell: &str) -> Cell {
    if !cell.contains('`') {
        return Cell::Broken("no fixture".to_owned());
    }
    let mut found: Vec<Group> = Vec::new();
    for mut words in cell.split("; ").map(backticked) {
        let Some(first) = words.first().cloned() else {
            return Cell::Broken("an empty group".to_owned());
        };
        // entries only: no file in the group, and its first word is no folder of an earlier group: more entries of the file before
        if !found.is_empty()
            && !words.iter().any(|w| is_file(w))
            && !found.iter().any(|g| g.folder == first)
        {
            if let Some(before) = found.last_mut() {
                before.entries.extend(words);
            }
            continue;
        }
        let folder = if is_file(&first) {
            match found.last() {
                Some(before) => before.folder.clone(),
                None => return Cell::Broken(format!("the file `{first}` has no folder")),
            }
        } else {
            words.remove(0)
        };
        if words.is_empty() {
            return Cell::Broken(format!("the folder `{folder}` has no file"));
        }
        let file = words.remove(0);
        if !is_file(&file) {
            return Cell::Broken(format!(
                "`{file}` is where the file of `{folder}` should be"
            ));
        }
        found.push(Group {
            folder,
            file,
            entries: words,
        });
    }
    Cell::Groups(found)
}

/// `name#n`: the qualname and which of the defs of that name.
pub(crate) fn entry_name(entry: &str) -> (&str, usize) {
    match entry.rsplit_once('#') {
        Some((name, n)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => {
            (name, n.parse().unwrap_or(0))
        }
        _ => (entry, 0),
    }
}

/// What is wrong with a table, one sentence for each. `fixtures` is the folder of the rows' fixture folders, `snapshots` the folder of their snapshots.
pub(crate) fn problems(
    table: &str,
    header: &str,
    fixtures: &Path,
    snapshots: &Path,
) -> Vec<String> {
    let rows = rows(table, header);
    let mut found = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for row in &rows {
        let id = &row.id;
        if id.is_empty() || seen.contains(&id.as_str()) {
            found.push(format!("row `{id}`: the id is empty or used twice"));
        }
        seen.push(id);
        match &row.cell {
            Cell::Broken(why) => found.push(format!("row `{id}`: {why}")),
            Cell::Groups(groups) => {
                for group in groups {
                    let root = fixtures.join(&group.folder);
                    if !root.is_dir() {
                        found.push(format!(
                            "row `{id}`: the folder `{}` is not there",
                            group.folder
                        ));
                    } else if !root.join(&group.file).is_file() {
                        found.push(format!(
                            "row `{id}`: the file `{}` is not in `{}`",
                            group.file, group.folder
                        ));
                    }
                    if group.entries.is_empty() {
                        found.push(format!(
                            "row `{id}`: the file `{}` has no entry",
                            group.file
                        ));
                    }
                    for entry in &group.entries {
                        if entry_name(entry).1 > 0 {
                            found.push(format!(
                                "row `{id}`: the entry `{entry}` is not the def `call_graph` enters for that name"
                            ));
                        }
                    }
                }
                if !snapshots.join(format!("{id}.txt")).is_file() {
                    found.push(format!("row `{id}`: no snapshot"));
                }
            }
        }
    }
    if let Ok(dir) = fs::read_dir(snapshots) {
        let mut names: Vec<String> = dir
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            let id = name.strip_suffix(".txt").unwrap_or(&name);
            if !rows
                .iter()
                .any(|r| r.id == id && matches!(r.cell, Cell::Groups(_)))
            {
                found.push(format!("snapshot `{name}`: no row of the table builds it"));
            }
        }
    }
    found
}
