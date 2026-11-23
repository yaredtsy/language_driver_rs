// Kinds the totality walk of `sem/view/tests.rs` must meet that no other fixture has: a spread argument, a JSX spread attribute.
function handler() {}
function many(...fs: (() => void)[]) { fs.forEach((f) => f()); }
function Panel(props: { onClose: () => void }) { return null; }

export function kinds() {
  many(...[handler]);
  return <Panel {...{ onClose: handler }} />;
}
