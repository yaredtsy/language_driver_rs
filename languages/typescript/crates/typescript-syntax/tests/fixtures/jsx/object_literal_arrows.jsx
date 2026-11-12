// D21 in a component file: handlers held by a `const` object are defs, props passed to an element are not.
const handlers = {
  onSave: () => {},
};

export function Panel() {
  return <button {...{ onBlur: () => {} }} />;
}
