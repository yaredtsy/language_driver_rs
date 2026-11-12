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
