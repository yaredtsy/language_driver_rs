import { Base, Tool, helper } from "../../shared/lib";
import { typed } from "../../shared/api";
import { local } from "./local";

export function main() {
  helper();
  typed();
  new Tool().run();
  local();
}

class Mine extends Base {}

export function build() {
  new Mine();
}

export function hands() {
  take(helper);
}

function take(f: () => void) {
  f();
}
