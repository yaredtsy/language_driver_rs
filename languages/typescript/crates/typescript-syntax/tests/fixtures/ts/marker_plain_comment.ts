/* a plain block */
function afterPlainBlock() {}

// a line comment
function afterLineComment() {}

/* ID: not-a-marker */
function plainBlockWithLabel() {}

// ID: not-a-marker-either
function lineWithLabel() {}

/** doc above a line comment */
// note between
function jsdocThenLine() {}

/** doc above a plain block */
/* note between */
function jsdocThenBlock() {}
