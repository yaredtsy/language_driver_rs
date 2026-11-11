import { double } from "./util";

export function total(xs: number[]): number {
  return xs.map(double).reduce((a, b) => a + b, 0);
}
