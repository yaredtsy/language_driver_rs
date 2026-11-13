declare function require(id: string): any;
declare function make(): new () => object;
declare class Ambient extends make() {}

export async function load(name: string) {
  const m = await import("./top-level");
  const r = require("./enum");
  const n = require(name);
  return [m, r, n];
}
