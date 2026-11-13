declare function f(): number;
declare function key(): string;
declare function dec(...args: any[]): any;

export namespace N {
  export function inner() {
    f();
  }
  @dec
  export class C {
    static s = f();
    [key()] = 1;
    x = f();
    m() {
      f();
    }
  }
  f();
}
