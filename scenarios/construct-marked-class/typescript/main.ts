// A module, as TypeScript files are: a script would merge its functions with the DOM's globals.
export {};

/**
 * A class with a marker and a constructor.
 *
 * ID: 11111111-0000-0000-0000-000000000001
 */
class Report {
  constructor() {
    prepare();
  }
}

/** ID: 11111111-0000-0000-0000-000000000002 */
class Plain {}

function prepare(): number {
  return 1;
}

function main() {
  new Report();
  new Plain();
}
