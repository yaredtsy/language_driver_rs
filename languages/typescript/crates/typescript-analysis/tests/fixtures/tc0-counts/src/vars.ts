function handler() {}
function other() {}

export function locals(flag: boolean, xs: (() => void)[], o: { k: () => void }, cb: () => void) {
  const a = handler;
  let b = handler;
  let c;
  c = other;
  let d = handler;
  d = other;
  let e = handler;
  ({ k: e } = o);
  let f = handler;
  for (f of xs) {}
  let n = 1;
  n = 2;
  const { k } = o;
  xs.forEach((x) => { let g = handler; g = other; x(); });
  function nested() { let h = handler; h = other; return h; }
  if (flag) { cb = handler; }
  return [a, b, c, d, e, f, n, k, nested];
}
