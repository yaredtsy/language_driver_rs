// A module, as TypeScript files are: a script would merge its functions with the DOM's globals (`fetch`).
export {};

function store() {
  return 3;
}

function fetch() {
  return parse();
}

function parse() {
  return 1;
}

function transform() {
  return 2;
}

function main() {
  fetch();
  transform();
  store();
}
