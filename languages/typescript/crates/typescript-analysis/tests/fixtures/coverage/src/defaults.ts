declare function f(): number;
declare function g(): { a: number };

export function withDefault(x = f(), { a = f() } = g()) {
  return x + a;
}

export class K {
  constructor(public p = f()) {}
}

export const arrow = (y = f()) => y;
