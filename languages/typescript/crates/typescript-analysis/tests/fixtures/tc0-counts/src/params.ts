interface Writer { write(): void }
type Props = { onClose: () => void; actions: { close: () => void; deep: { go: () => void } } };

export function own(cb: () => void) { cb(); }
export function viaMember(props: Props) { props.onClose(); }
export function viaPattern({ onClose }: Props) { onClose(); }
export function deep(props: Props) { props.actions.close(); }
export function deeper(props: Props) { props.actions.deep.go(); }
export function patternAndMember({ actions }: Props) { actions.close(); }
export function method(w: Writer) { w.write(); }
export function outer(cb: () => void) {
  function inner() { cb(); }
  return inner;
}
export function anon(xs: (() => void)[]) { xs.forEach((f) => f()); }
