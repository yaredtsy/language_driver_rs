declare function run(...values: unknown[]): void;
declare let n: number;
declare const o: { k?: number };
export function kinds() {
  run("s", 1, 1n, /r/, `t`, `t${n}`, true, false, null, -n, n++, typeof n, void 0, delete o.k, <div></div>, <br />, <></>);
  run(undefined);
}
