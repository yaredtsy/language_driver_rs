// What a destructuring takes from an object that holds defs, through a pattern no edge follows: a rest element, an assignment target. Counted when the
// name it binds is handed over (`use(name)`), one for each place.
declare function use(value: unknown): void;

const api = {
  load() {},
  save() {},
  label: "api",
};
const plain = { count: 1, label: "plain" };

// the rest object holds `save`, a def: handed over by `use(others)`, and no edge is drawn for it: 1
export function restTakesFunctions() {
  const { load, ...others } = api;
  use(others);
}
// one for each place it is handed over: 2
export function restHandedTwice() {
  const { load, ...others } = api;
  use(others);
  use(others);
}
// a rest object that holds no def (`count`, `label`): 0
export function restHoldsNoDef() {
  const { count, ...others } = plain;
  use(others);
}
// every def is named by the pattern, so what is left is `label`: 0
export function restHoldsOnlyWhatIsNamed() {
  const { load, save, ...others } = api;
  use(others);
}
// a rest object that is not handed over: `others.save()` is an ordinary call, an edge: 0
export function restIsCalled() {
  const { load, ...others } = api;
  others.save();
}
// a rest object that nothing uses: 0
export function restIsNotUsed() {
  const { load, ...others } = api;
}
// a named element is followed, not counted here (the edge to `load`): 0
export function namedElement() {
  const { load } = api;
  use(load);
}

// `load` is assigned from `api.load`: handed over by `use(load)`: 1
export function assignedAndHanded() {
  let load: () => void;
  ({ load } = api);
  use(load);
}
// a renamed target, handed over twice: 2
export function assignedRenamedHandedTwice() {
  let go: () => void;
  ({ load: go } = api);
  use(go);
  use(go);
}
// a nested pattern: 1
const deep = { inner: { run() {} } };
export function assignedNested() {
  let run: () => void;
  ({ inner: { run } } = deep);
  use(run);
}
// the target is called, not handed over (a call through a value: `function_typed_value`, counted there): 0
export function assignedAndCalled() {
  let load: () => void;
  ({ load } = api);
  load();
}
// a member that is not a def: 0
export function assignedNotAFunction() {
  let count: number;
  ({ count } = plain);
  use(count);
}
// assigned and never used: 0
export function assignedNeverUsed() {
  let load: () => void;
  ({ load } = api);
}
// an ordinary assignment is not a destructuring: 0
export function assignedPlainly() {
  let load: () => void;
  load = api.load;
  use(load);
}
// a comparison with an object literal on its left is not an assignment: 0
export function comparedNotAssigned() {
  let load: () => void;
  ({ load } == api);
  use(load);
}

// ---- a rest in an assignment pattern: the rest object takes the members the pattern did not name
// `load` and `save` are defs and none is named: 1
export function assignedRestHanded() {
  let o: typeof api;
  ({ ...o } = api);
  use(o);
}
// handed over twice: 2
export function assignedRestHandedTwice() {
  let o: typeof api;
  ({ load: _, ...o } = api);
  use(o);
  use(o);
}
// the rest object holds `label` only, a member that is not a def: 0
export function assignedRestHoldsNoDef() {
  let o: typeof plain;
  ({ ...o } = plain);
  use(o);
}
// every def is named by the pattern: 0
export function assignedRestOnlyWhatIsNamed() {
  let o: typeof api;
  let load, save;
  ({ load, save, ...o } = api);
  use(o);
}
// the rest object is not handed over, and never used: 0
export function assignedRestNotHanded() {
  let o: typeof api;
  ({ ...o } = api);
}
declare let _: unknown;
// two patterns assign to one variable: it is one variable handed over once: 1
export function assignedRestTwice() {
  let o: typeof api;
  ({ ...o } = api);
  ({ ...o } = api);
  use(o);
}

// ---- a `for … of` pattern: the name stands for a member of the element, and nothing follows it
declare const apis: (typeof api)[];
declare const decks: (typeof deep)[];
declare const plains: (typeof plain)[];
// `load` is a def of every element: 1
export function forOfHanded() {
  for (const { load } of apis) use(load);
}
// handed over twice: 2
export function forOfHandedTwice() {
  for (const { load } of apis) {
    use(load);
    use(load);
  }
}
// a renamed element: 1
export function forOfRenamed() {
  for (const { save: keep } of apis) use(keep);
}
// a nested pattern: 1
export function forOfNested() {
  for (const { inner: { run } } of decks) use(run);
}
// `await` does not change it: 1
export async function forOfAwaited() {
  for await (const { load } of apis) use(load);
}
// called, not handed over: the engine follows the binding to `api.load` for a call (an edge), so nothing is counted: 0
export function forOfCalled() {
  for (const { load } of apis) load();
}
// a member that is not a def: 0
export function forOfNotAFunction() {
  for (const { count } of plains) use(count);
}
// never used: 0
export function forOfUnused() {
  for (const { load } of apis) {
  }
}
// a destructuring that has an initializer is not a `for … of`: it is followed (an edge, counted nowhere here): 0
export function initializerNotForOf() {
  const { load } = api;
  use(load);
}
// a rest element in a `for … of` pattern: the rest object holds `load` and `save`: a rest element (1), and not a member named `load`
export function forOfRest() {
  for (const { ...load } of apis) use(load);
}
// an array pattern is not followed (its element is named like a member on purpose, which an object pattern would take): 0
export function forOfArrayPattern() {
  // @ts-ignore
  for (const [load] of apis) use(load);
}
