import { helper } from "pkg";
import { other } from "@scope/other";
import { fake } from "fake";
import type { Shape } from "./types";

export function run(shape: Shape): number {
  return helper() + other() + fake() + shape.sides;
}
