/** Overloads with an implementation are one def. */
export function dup(a: string): string;
export function dup(a: number): number;
export function dup(a: any): any {
  return a;
}

/** Signatures with no implementation are one def each, with one qualname. */
export declare function ambient(a: string): string;
export declare function ambient(a: number): number;

export class Box {
  open() {}
  get size() {
    return 1;
  }
  set size(value: number) {}
}

export const arrow = () => {};

function outer() {
  function inner() {}
  return inner;
}
