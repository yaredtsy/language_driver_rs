// An overloaded function: one symbol, three declarations, one body.
export function over(a: string): void;
export function over(a: number): void;
export function over(a: unknown) {}

export function callsOver(flag: boolean) {
  over(1);
  (flag ? over : over)(2);
}
