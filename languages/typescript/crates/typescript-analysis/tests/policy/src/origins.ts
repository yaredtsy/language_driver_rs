// Where the function of a `function_typed_value` call comes from: the leftmost name of the callee (`tests/function_typed_origins.rs`).
import { useStore } from "./origins_lib";

// a parameter, a name taken out of one by a pattern, `props.go()`, and a callee under `!` and `?.`
export function fromParameter(
  cb: () => void,
  props: { go: () => void },
  { onClose }: { onClose: () => void },
  maybe?: { go?: () => void },
) {
  cb();
  props.go();
  onClose();
  (maybe!)?.go?.();
}

declare function useState(): [number, () => void];
declare function create(): () => void;
// a variable, or a name taken out of one by a pattern, whose initializer is a call
export function fromCallResult() {
  const [open, setOpen] = useState();
  setOpen();
  const makeIt = create();
  makeIt();
}

// through an import: the declaration the import leads to is a variable with a call as its initializer
export function fromImport() {
  useStore();
}

class Store {
  constructor(public handler: () => void) {}
  run() {
    this.handler();
  }
}
export { Store };

// a variable that holds an arrow, and one that is only declared: neither is a parameter or a call's result
declare const declared: () => void;
export function fromOther() {
  let later = () => {};
  later();
  declared();
}
