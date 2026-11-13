declare function f(): number;
declare function key(): string;
declare function dec(...args: any[]): any;
declare function base(): new () => object;

export function make() {
  @dec
  class Local extends base() {
    static s = f();
    t = f();
    static {
      f();
    }
    [key()] = 1;
    @dec
    decorated() {}
    param(@dec x: number) {}
    method() {
      f();
    }
    constructor() {
      super();
      f();
    }
  }
  return new Local();
}
