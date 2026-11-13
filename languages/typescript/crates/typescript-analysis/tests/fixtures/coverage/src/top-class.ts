declare function f(): number;
declare function key(): string;
declare function dec(...args: any[]): any;
declare function base(): new () => object;

@dec
export class Top extends base() {
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

export function build() {
  return new Top();
}
