// A module, as TypeScript files are: a script would merge its functions with the DOM's globals.
export {};

// `each` calls whatever `f` its caller hands it: a def by name, or an arrow written at the call.
function work(x: number): number {
  return x;
}

function each(f: (x: number) => number, x: number): number {
  return f(x);
}

function main(): number {
  const a = each(work, 1);
  return each((x) => work(x), a);
}
