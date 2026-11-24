// Every kind of function-like def whose parameters the table reads (TC1 step 2).
export class Point {
  constructor(public x: number, ...more: number[]) {}
  get size(): number {
    return 0;
  }
  set size(value: number) {}
  move(this: Point, by: number) {}
}
