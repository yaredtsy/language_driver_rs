declare function register(o: unknown): void;

function Close() {
  return null;
}

// a handler given to an element's attribute, and one inside a spread attribute: both are references by name; the element's own tag is not (step 6 makes it a call)
export function Panel() {
  const handleClose = () => {};
  const onKey = () => {};
  return (
    <button onClick={handleClose} {...{ onKeyDown: onKey }}>
      <Close />
    </button>
  );
}
