// A module, as TypeScript files are: a script would merge its functions with the DOM's globals (`fetch`).
export {};

class Counter {
  bump(): number {
    return this.read();
  }

  read(): number {
    return 1;
  }

  static reset(): number {
    return 0;
  }
}

declare const counter: Counter;

function main() {
  counter.bump();
  Counter.reset();
}
