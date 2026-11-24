// The body as data (TC1 step 2): each argument and callee of `flow`, read into an `Expr`.
declare function run(...values: unknown[]): void;
declare const flag: boolean;

export function target(a: number, ...rest: string[]) {}
export const held = () => {};
export class Shape {}
function Card(props: { title: string }) {
  return null;
}

export function flow(this: unknown, p: () => void, q?: number) {
  target(1, "s", ...["a"]);
  run(p as unknown, held!, (held), held satisfies () => void);
  run(() => target(2));
  run(function () {}, function named() {});
  run(flag ? held : target, p || held, p ?? held, p && held);
  run(-1, typeof p, 1 + 2, `t${q}`, <i />, null, true);
  run(Shape, new Shape(), [held], { held }, p = held, q);
  <Card title="x" />;
  run(() => run(() => target(3)));
}
