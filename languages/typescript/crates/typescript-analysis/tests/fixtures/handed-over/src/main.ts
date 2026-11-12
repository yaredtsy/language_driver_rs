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

// ---- a value that holds defs, handed over (D22 step 4): a literal in a hand-over position, or a name that holds a literal; its function members go with it
export function inPlaceLiteral() {
  // `m` is a def (a method); `a` is an arrow in an argument's literal, which is not a def (D21), so only `m` is handed over
  register({ m() {}, a: () => {} });
}
// Q-TS2: what a method of an argument's literal calls was in no tree
export function argMethodCalls() {
  register({
    onSuccess() {
      sharedHelper();
    },
  });
}
export function inPlaceNestedLiteral() {
  register({ data: { m() {} } });
}
export function literalInReturn() {
  return { m() {} };
}
export function literalInArray() {
  register([{ m() {} }]);
}
export function literalByName() {
  // both members go with the name, at the name
  const o = { m() {}, a: () => {}, plain: 3 };
  register(o);
}
export function nestedLiteralByName() {
  const o = { data: { onToggle: () => {} } };
  register(o);
}
export function nestedLiteralByAccess() {
  const o = { data: { onToggle: () => {} } };
  register(o.data);
}
export function wrappedLiteralByName() {
  const o = { m() {} } as const;
  register(o);
}
export function literalByNameTwice() {
  const o = { m() {} };
  register(o);
  register(o);
}
export function literalCalledAndPassed() {
  // `m` is called through the name, and handed over with the literal: both edges
  const o = { m() {} };
  o.m();
  register(o);
}
export function literalPassedInAClosure() {
  const o = { m() {} };
  function inner() {
    register(o);
  }
  inner();
}
export function importedLiteralByName() {
  register(api);
}
// not handed over: a literal that is made and kept, a literal nothing names, a `let` literal's arrow (not a def), a literal that is only read
export function deadLiteral() {
  const dead = { a: () => {}, m() {} };
}
export function letLiteral() {
  let o = { a: () => {} };
  register(o);
}
export function literalRead() {
  const o = { m() {} };
  if (o) {
    register(1);
  }
  register(typeof o);
}
export function boundNamedFunctionExpression() {
  // bound to a variable, and a property of a literal nobody hands over: made, not handed over
  const g = function step() {};
  const o = { cb: function inner() {} };
}
export function storedLiteralNotHandedOver() {
  // a nested literal of a literal nobody hands over
  const x = { data: { m() {} } };
}

// ---- where a name is handed over: an argument, a property value, an array element, an initializer, an assignment, a return, a JSX value, through wrappers
declare class Box {
  constructor(f: unknown);
}
declare const holder: { cb: unknown };

export function inArray() {
  const handler = () => {};
  register([handler]);
}
export function inVariable() {
  const handler = () => {};
  const alias = handler;
  register(alias);
}
export function inReturn() {
  const handler = () => {};
  return handler;
}
export function inConciseArrow() {
  const handler = () => {};
  register(() => handler);
}
export function inConditional() {
  const a = () => {};
  const b = () => {};
  register(flag ? a : b);
}
export function inLogical() {
  const handler = () => {};
  register(flag && handler);
}
export function inWrappers() {
  const handler = () => {};
  register(handler as unknown);
  register((handler));
  register(handler!);
}
export function inAssignment() {
  const handler = () => {};
  holder.cb = handler;
}
export function inNew() {
  const handler = () => {};
  new Box(handler);
}
export function inProperty() {
  const handler = () => {};
  register({ cb: handler });
}
// the same def named twice in one body: two hand-overs, each at its own site
export function namedTwice() {
  const handler = () => {};
  register(handler);
  register(handler);
}

// a hook's dependency array lists what the hook watches; by the rule an array element handed to a call is a hand-over, so both are passed edges. A known
// over-report, kept as it is (see ENGINE.md): this is its current answer
declare function useEffect(f: () => void, dependencies: unknown[]): void;
export function inDependencyArray() {
  const a = () => {};
  const b = () => {};
  useEffect(() => {}, [a, b]);
}

// ---- a name that is read, not handed over: no edge
export function readNotHandedOver() {
  const handler = () => {};
  if (handler) {
    register(1);
  }
  register(typeof handler === "function");
  register(handler === sharedHelper);
  register(!handler);
  register(handler ? 1 : 2);
  register(handler && 1);
  register(handler.length);
  register(`${handler}`);
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
