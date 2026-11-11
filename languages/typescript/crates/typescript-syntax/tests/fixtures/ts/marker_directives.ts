// @ts-ignore
function ignored() {}

// eslint-disable-next-line no-console
function linted() {}

/* prettier-ignore */
function formatted() {}

// a plain note
function noted() {}

// eslint-disable-next-line
// a note in between
function directiveThenNote() {}

// @ts-ignore

function separatedByBlank() {}

/* eslint-disable no-console */
function rangeDisable() {}

// @ts-nocheck
function filePragma() {}

// deno-lint-ignore no-explicit-any
function denoLinted() {}

// @ts-expect-error
// a note the compiler looks past
function reachedThroughANote() {}
