export function leaf() {}
export function other() {}
function each(xs: number[], f: (n: number) => void) {
  for (const x of xs) f(x);
}
// an anonymous function's calls belong to the def around it, where they are written
export function callbacks(xs: number[]) {
  each(xs, (n) => leaf());
  each(xs, function (n) {
    other();
  });
  return () => other();
}
