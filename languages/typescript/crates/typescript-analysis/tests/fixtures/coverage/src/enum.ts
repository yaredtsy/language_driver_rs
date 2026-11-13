declare function f(): number;

export enum E {
  A = f(),
  B = f() + 1,
  C = 3,
}
export const enum K {
  X = f(),
}
