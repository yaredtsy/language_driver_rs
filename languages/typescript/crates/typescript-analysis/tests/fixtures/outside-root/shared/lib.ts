export function helper() {
  inner();
}
function inner() {}

export class Tool {
  constructor() {
    setup();
  }
  run() {}
}

export class Base {
  constructor() {
    setup();
  }
}
function setup() {}
