import { b } from "./other";

/** ID: SAME */
function a() {
  x();
}

function x() {}

export function main() {
  a();
  b();
}
