/**
 * Doc above a note.
 * ID: through-a-note
 */
// a note between
function markedThroughNote() {}

/** ID: through-a-directive */
// eslint-disable-next-line no-unused-vars
function markedThroughDirective(x: number) {}

/** Doc above a directive. */
// @ts-ignore
function unmarkedThroughDirective() {}

/** ID: same-line */ // why
function trailingLineComment() {}

/** ID: too-far */
// a note, then a blank line

function blankAfterNote() {}
