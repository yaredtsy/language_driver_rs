// D21 in a component file: handlers held by a `const` object are defs, props passed to an element are not.
const handlers = {
  onSave: () => {},
  nested: { onClose: async () => {} },
};

export function Panel() {
  const local = { onClick: () => {} } as const;
  return <button onClick={local.onClick} {...{ onBlur: () => {} }} />;
}
