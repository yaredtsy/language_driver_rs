// A module, as TypeScript files are: a script would merge its functions with the DOM's globals.
export {};

function local(): number {
  return 1;
}

function main() {
  Math.max(1, 2);
  local();
}
