//! Reading (and, in `plan`, placing) a marker in the JSDoc block above a def. Plain text work on one comment: no engine type is named here,
//! so that a language with the same `/** … */` shape can share it by the rule of two.
//!
//! The rule is `docs/03-typescript/04-ts2-markers.md` §3 and `languages/typescript/docs/markers.md`.

use analyzer_core::edit::{Edit, block_marker, comment_marker};
use analyzer_core::text::Span;

use crate::Skip;
use crate::sem::{Above, Comment, CommentKind};

/// The JSDoc block that documents a def: the nearest `/** */` block among the comments above it. Blank lines and `//` or `/* */` comments
/// between the block and the def do **not** detach it: this is the block TypeScript applies to the def, and the only one it applies (a
/// second block written below it would hide the first, and switch its tags off).
///
/// One block is not the def's: the file's opening comment (`file_comment`). It is the file's, whatever stands below it.
pub(crate) fn doc_above(text: &str, above: &Above) -> Option<Span> {
    let nearest = above
        .comments
        .iter()
        .rposition(|c| c.kind == CommentKind::Jsdoc)?;
    (!in_file_comment(text, above, nearest)).then_some(above.comments[nearest].span)
}

/// The nearest JSDoc block above a def when it is part of the file's opening comment, and so not the def's.
fn file_comment(text: &str, above: &Above) -> Option<Span> {
    let nearest = above
        .comments
        .iter()
        .rposition(|c| c.kind == CommentKind::Jsdoc)?;
    in_file_comment(text, above, nearest).then_some(above.comments[nearest].span)
}

/// Is `comments[i]` in the file's opening comment: the comments from the top of the file down to the first blank line, when that blank
/// line comes before the def? A licence, an overview. When no blank line stands between the top of the file and the def, the comments are
/// the def's own.
fn in_file_comment(text: &str, above: &Above, i: usize) -> bool {
    if !above.opens_file {
        return false;
    }
    let comments = &above.comments;
    // the first comment with a blank line after it ends the opening comment
    let end = (0..comments.len()).find(|&k| {
        let next = comments.get(k + 1).map_or(above.start, |c| c.span.start);
        !comment_marker::adjacent(text, comments[k].span.end, next)
    });
    end.is_some_and(|end| i <= end)
}

/// Does a block comment's text carry a block tag: a line that, after its leading `*` and spaces, starts with `@`? An inline `{@link …}` is
/// not one.
fn has_block_tag(block: &str) -> bool {
    block_marker::lines(block)
        .iter()
        .enumerate()
        .any(|(i, line)| {
            let s = line.trim();
            let s = if i == 0 {
                s.strip_prefix("/**").unwrap_or(s)
            } else {
                s.strip_prefix('*').unwrap_or(s)
            };
            s.trim_start().starts_with('@')
        })
}

/// The marker on a def, or `None`: the def must own its line, have a JSDoc block above it (`doc_above`), and the block must carry a
/// `<label>:` line.
pub(crate) fn marker_of(text: &str, above: &Above, label: &str) -> Option<String> {
    comment_marker::own_line(text, above.start)?;
    let block = doc_above(text, above)?;
    block_marker::read(block.slice(text)?, label)
}

/// The compiler's own directives. They apply to the next line that is neither blank nor a `//` comment, so they reach a def through blank
/// lines and `//` comments, and a block comment in between cuts them off.
const COMPILER_DIRECTIVES: [&str; 2] = ["@ts-ignore", "@ts-expect-error"];

/// Tool comments that apply to the line or statement directly below them: a block put between one of these and the def would move what
/// it points at. Comments for a whole file or a range (`eslint-disable`, `@ts-nocheck`, `deno-lint-ignore-file`) are not here: a block
/// below them changes nothing.
const NEXT_LINE_DIRECTIVES: [&str; 21] = [
    "eslint-disable-next-line",
    "oxlint-disable-next-line",
    "tslint:disable-next-line",
    "biome-ignore",
    "rome-ignore",
    "deno-lint-ignore",
    "prettier-ignore",
    "dprint-ignore",
    "istanbul ignore next",
    "istanbul ignore if",
    "istanbul ignore else",
    "c8 ignore next",
    "v8 ignore next",
    "node:coverage ignore next",
    "noinspection",
    "$FlowFixMe",
    "$FlowExpectedError",
    "$FlowIgnore",
    "flowlint-next-line",
    "cspell:disable-next-line",
    "nosemgrep",
];

/// Does the comment's text start with one of `words`, as a whole word? `deno-lint-ignore-file` does not start with the word
/// `deno-lint-ignore`, and `eslinting` is not `eslint`.
fn starts_with_word(comment: &str, words: &[&str]) -> bool {
    let body = comment
        .trim_start_matches('/')
        .trim_start_matches('*')
        .trim_end_matches("*/")
        .trim();
    words.iter().any(|word| {
        body.strip_prefix(word)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '-'))
    })
}

/// Is this comment a directive that applies to the line or statement below it?
pub(crate) fn is_directive(comment: &str) -> bool {
    starts_with_word(comment, &COMPILER_DIRECTIVES)
        || starts_with_word(comment, &NEXT_LINE_DIRECTIVES)
}

/// Does a comment above the def point at the def's first line, so that a new block between them would move its target?
fn directive_above(text: &str, above: &Above) -> bool {
    let written = |c: &Comment| c.span.slice(text).unwrap_or("");
    // a tool's directive applies to the next line: only the comment directly above the def can point at it
    if let Some(last) = above.comments.last()
        && comment_marker::adjacent(text, last.span.end, above.start)
        && is_directive(written(last))
    {
        return true;
    }
    // the compiler's reaches further: up from the def's line, past blank lines and `//` comments
    for c in above.comments.iter().rev() {
        if starts_with_word(written(c), &COMPILER_DIRECTIVES) {
            return true;
        }
        if c.kind != CommentKind::Line {
            break;
        }
    }
    false
}

/// The edit that puts `marker_text` on the def that `above` is about, or why not. `errors` are the file's parse errors.
pub(crate) fn plan(
    text: &str,
    above: &Above,
    def_has_error: bool,
    errors: &[Span],
    label: &str,
    marker_text: &str,
) -> Result<Edit, Skip> {
    let start = above.start;
    let own = comment_marker::own_line(text, start).ok_or(Skip::NotOnOwnLine)?;
    // the marker goes inside a `/** */` block: it must also not close the comment
    if !comment_marker::fits_a_line(marker_text) {
        return Err(Skip::InvalidText);
    }
    let block = doc_above(text, above);
    let touches = |b: Span| {
        errors
            .iter()
            .any(|e| e.start < b.end && e.end.max(e.start + 1) > b.start)
    };
    if def_has_error || block.is_some_and(touches) {
        return Err(Skip::SyntaxError);
    }
    if let Some(existing) = block
        .and_then(|b| b.slice(text))
        .and_then(|b| block_marker::read(b, label))
    {
        return Err(Skip::AlreadyMarked { existing });
    }
    let eol = comment_marker::line_ending(text);
    match block {
        Some(block) => {
            // a one-line block that is itself a directive (`/** @ts-ignore */`) stops being one when it is rewritten as several lines
            if block.slice(text).is_some_and(is_directive) {
                return Err(Skip::DirectiveAbove);
            }
            plan_in_block(text, block, label, marker_text)
        }
        None => {
            // the file's opening comment is not written into (it is a licence, an overview), but the engine applies it to the def all
            // the same. With block tags neither way is safe: they may be the def's (in a `.js` file, its types), and a new block below
            // would switch them off
            if file_comment(text, above)
                .and_then(|block| block.slice(text))
                .is_some_and(has_block_tag)
            {
                return Err(Skip::FileCommentAbove);
            }
            // a new block goes directly above the def, below any plain comment: unless that comment points at the def
            if directive_above(text, above) {
                return Err(Skip::DirectiveAbove);
            }
            let indent = &text[own as usize..start as usize];
            Ok(Edit::insert(
                own,
                format!("{indent}/** {label}: {marker_text} */{eol}"),
            ))
        }
    }
}

/// Put the marker in an existing JSDoc block. A multi-line block gets a ` * <label>: <text>` line before its first block tag, or before the
/// closing line when it has none (core's `block_marker::plan`). A one-line block is rewritten as several lines.
fn plan_in_block(text: &str, block: Span, label: &str, marker_text: &str) -> Result<Edit, Skip> {
    let body = block.slice(text).ok_or(Skip::UnsafeDocComment)?;
    if !body.contains(['\n', '\r']) {
        return one_line(text, block, body, label, marker_text);
    }
    block_marker::plan(text, block, label, marker_text, true, false).ok_or(Skip::UnsafeDocComment)
}

fn one_line(
    text: &str,
    block: Span,
    body: &str,
    label: &str,
    marker_text: &str,
) -> Result<Edit, Skip> {
    let line_start = comment_marker::own_line(text, block.start).ok_or(Skip::NotOnOwnLine)?;
    let indent = &text[line_start as usize..block.start as usize];
    let eol = comment_marker::line_ending(text);
    let content = body
        .strip_prefix("/**")
        .and_then(|b| b.strip_suffix("*/"))
        .ok_or(Skip::UnsafeDocComment)?
        .trim();
    let marker_line = format!("{indent} * {label}: {marker_text}");
    let mut lines = vec!["/**".to_owned()];
    if content.is_empty() {
        lines.push(marker_line);
    } else if content.starts_with('@') {
        // the marker ends the description: it goes before a block tag
        lines.push(marker_line);
        lines.push(format!("{indent} * {content}"));
    } else {
        lines.push(format!("{indent} * {content}"));
        lines.push(marker_line);
    }
    lines.push(format!("{indent} */"));
    Ok(Edit::replace(block, lines.join(eol)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jsdoc(text: &str) -> Comment {
        let start = text.find("/**").unwrap() as u32;
        let end = text[start as usize..].find("*/").unwrap() as u32 + start + 2;
        Comment {
            span: Span::new(start, end),
            kind: CommentKind::Jsdoc,
        }
    }

    fn def_start(text: &str) -> u32 {
        text.find("function").unwrap() as u32
    }

    fn above(text: &str, comments: &[Comment], opens_file: bool) -> Above {
        Above {
            start: def_start(text),
            comments: comments.to_vec(),
            opens_file,
        }
    }

    fn line(text: &str) -> Comment {
        let start = text.find("//").unwrap() as u32;
        let end = text[start as usize..].find('\n').unwrap() as u32 + start;
        Comment {
            span: Span::new(start, end),
            kind: CommentKind::Line,
        }
    }

    #[test]
    fn a_block_directly_above_is_the_defs() {
        for opens_file in [true, false] {
            let text = "/** doc */\nfunction f() {}\n";
            assert_eq!(
                doc_above(text, &above(text, &[jsdoc(text)], opens_file)),
                Some(jsdoc(text).span)
            );
            let same_line = "/** doc */ function f() {}\n";
            assert!(
                doc_above(
                    same_line,
                    &above(same_line, &[jsdoc(same_line)], opens_file)
                )
                .is_some()
            );
        }
    }

    #[test]
    fn a_blank_line_does_not_detach_the_block() {
        // further down the file: the engine applies the block to the def, so it is the def's
        for text in [
            "/** doc */\n\nfunction f() {}\n",
            "/** doc */\r\n\r\nfunction f() {}\r\n",
            "/** doc */\n   \n\n\nfunction f() {}\n",
        ] {
            assert_eq!(
                doc_above(text, &above(text, &[jsdoc(text)], false)),
                Some(jsdoc(text).span),
                "{text:?}"
            );
        }
    }

    #[test]
    fn another_comment_between_does_not_detach_the_block() {
        for text in [
            "/** doc */\n// note\nfunction f() {}\n",
            "/** doc */\n\n// note\nfunction f() {}\n",
            "/** doc */\n// note\n\nfunction f() {}\n",
        ] {
            let comments = [jsdoc(text), line(text)];
            assert_eq!(
                doc_above(text, &above(text, &comments, false)),
                Some(jsdoc(text).span),
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_files_opening_comment_is_not_the_defs() {
        // at the top of the file, down to the first blank line
        for text in [
            "/** About the file. */\n\nfunction f() {}\n",
            "/** About the file. */\n\n// note\nfunction f() {}\n",
            "/** About the file. */\n// its second line\n\nfunction f() {}\n",
        ] {
            let mut comments = vec![jsdoc(text)];
            if text.contains("//") {
                comments.push(line(text));
            }
            let at_top = above(text, &comments, true);
            assert_eq!(doc_above(text, &at_top), None, "{text:?}");
            assert_eq!(
                file_comment(text, &at_top),
                Some(jsdoc(text).span),
                "{text:?}"
            );
            // the same comments further down the file are the def's
            let further_down = above(text, &comments, false);
            assert_eq!(file_comment(text, &further_down), None, "{text:?}");
        }
        // no blank line between the top of the file and the def: the comments are the def's own
        let glued = "/** doc */\n// note\nfunction f() {}\n";
        let comments = [jsdoc(glued), line(glued)];
        assert_eq!(
            doc_above(glued, &above(glued, &comments, true)),
            Some(jsdoc(glued).span)
        );
        // a block below the opening comment is the def's
        let two = "/* licence */\n\n/** doc */\n\nfunction f() {}\n";
        let licence = Comment {
            span: Span::new(0, 13),
            kind: CommentKind::Block,
        };
        assert_eq!(
            doc_above(two, &above(two, &[licence, jsdoc(two)], true)),
            Some(jsdoc(two).span)
        );
    }

    #[test]
    fn a_plain_block_is_not_a_jsdoc() {
        let text = "/* ID: x */\nfunction f() {}\n";
        let plain = Comment {
            span: Span::new(0, 11),
            kind: CommentKind::Block,
        };
        assert_eq!(doc_above(text, &above(text, &[plain], false)), None);
        assert_eq!(doc_above(text, &above(text, &[], false)), None);
    }

    #[test]
    fn a_block_tag_starts_a_line() {
        assert!(has_block_tag("/** @type {number} */"));
        assert!(has_block_tag("/**\n * Doc.\n * @param x a value\n */"));
        assert!(has_block_tag("/**\n @returns x\n*/"), "no star");
        assert!(!has_block_tag("/** Doc. */"));
        assert!(!has_block_tag(
            "/**\n * See {@link f}.\n * mail me@example.com\n */"
        ));
    }

    #[test]
    fn a_directive_is_a_whole_word_at_the_start_of_a_comment() {
        for yes in [
            "// @ts-ignore",
            "/// @ts-expect-error: the reason",
            "/* @ts-ignore */",
            "/** @ts-ignore */",
            "// eslint-disable-next-line no-console",
            "/* istanbul ignore next */",
            "// c8 ignore next 3",
            "// $FlowFixMe[incompatible-type]",
            "// deno-lint-ignore no-explicit-any",
            "// noinspection JSUnusedGlobalSymbols",
        ] {
            assert!(is_directive(yes), "{yes}");
        }
        for no in [
            "// @ts-check",
            "// @ts-nocheck",
            "/* eslint-disable no-console */",
            "// eslint-enable",
            "// eslinting is done in CI",
            "// deno-lint-ignore-file",
            "// c8 ignore start",
            "// see @ts-ignore below",
            "/**\n * @ts-ignore\n */",
        ] {
            assert!(!is_directive(no), "{no}");
        }
    }
}
