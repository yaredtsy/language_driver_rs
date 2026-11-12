import { api, util } from "./lib";

declare function register(o: unknown): void;
function tag(s: TemplateStringsArray) {}
function decorate(target: unknown, context: unknown) {}
declare const flag: boolean;
declare const xs: number[];

class Widget {
  method() {}
}
declare const widget: Widget;

function sharedHelper() {}

// A def that is written inside an expression, or named, and handed over instead of called. One function per shape: the counts of each are the row.

// ---- written inside an expression
export function argMethod() {
  register({ onSuccess() {} });
}
export function localLiteral() {
  const local = { traverse: () => {} };
  register(local);
}
export function namedFunctionExpression() {
  xs.map(function step() {});
}
export function writtenAndNamed() {
  // counted once, as written
  const local = { a: () => {} };
  register(local.a);
}

export function writtenHereNamedInAClosure() {
  // written in this body, and named by another body of the tree (the closure): one def, counted once, as written
  const local = { a: () => {} };
  function inner() {
    register(local.a);
  }
  inner();
}

// ---- referenced by name
export function constArrowByName() {
  const handler = () => {};
  register(handler);
}
export function declByName() {
  function decl() {}
  register(decl);
}
export function importedByName() {
  register(util);
}
export function propertyArrowByName() {
  register(api.load);
}
export function methodByName() {
  register(widget.method);
}
export function shorthand() {
  const f = () => {};
  register({ f });
}

// ---- not counted: called, or not a def, or not a hand-over
export function neverUsed() {
  // a def of its own name that nothing names: dead code, not a hand-over
  const unused = () => {};
  function alsoUnused() {}
}
export function literalCalled() {
  const kept = { a: () => {} };
  kept.a();
}
export function constArrowCalled() {
  const both = () => {};
  register(both);
  both();
}
export function argArrowProperty() {
  register({ cb: () => {} });
}
export function anonymousArrow() {
  register(() => {});
}
export function taggedTemplate() {
  tag`x`;
}
// a callee that is not one name (the engine cannot resolve it) is still being called, not handed over
export function unresolvedCallee() {
  (flag ? sharedHelper : callsIt)();
}
// a decorator is called (step 6 makes it a site), not handed over
@decorate
export class Decorated {}
// handed over here and called in the same tree: not counted
export function calledElsewhereInTheTree() {
  register(sharedHelper);
  callsIt();
}
function callsIt() {
  sharedHelper();
}
