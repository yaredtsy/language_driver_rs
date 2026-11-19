// What a call reaches when the declaration the engine answers is a signature: written in a type literal, an interface, or a `declare global`, or an abstract
// method (D43). `tests/signatures.rs` says what each entry gives. A signature has no body, and stands where the context layer is meant to put what really runs.

// ---- the owner's two cases
type Ops = { existsSync(p: string): boolean };
export function ownerParameter(fs: Ops, p: string) {
  fs.existsSync(p);
}
type Pair = { a(p: string): void; b(p: string): void };
export const impl: Pair = {
  a(p) {},
  b(p) {
    this.a(p);
  },
};

// ---- a method signature in a type literal, wherever the literal is
export function parameterType(o: { run(): void }) {
  o.run();
}
type Nested = { inner: { run(): void } };
export function nestedMember(a: Nested) {
  a.inner.run();
}
type Overloaded = { m(x: string): void; m(x: number): void };
export function overloadsInLiteral(o: Overloaded) {
  o.m(1);
}
type Both = { m(): void } & { n(): void };
export function intersection(a: Both) {
  a.m();
  a.n();
}
export function constraint<T extends { m(): void }>(t: T) {
  t.m();
}
type OneMethod = { m(): void };
type Picked = Pick<OneMethod, "m">;
export function throughPick(p: Picked) {
  p.m();
}
type Optional = { m?(): void };
export function optionalMethod(o: Optional) {
  o.m?.();
}
interface Holder {
  inner: { run(): void };
}
export function literalInInterface(i: Holder) {
  i.inner.run();
}
declare const toolbox: { run(): void };
export function declaredConst() {
  toolbox.run();
}
declare function make(): { run(): void };
export function returnType() {
  make().run();
}
export function genericArgument(m: Map<string, { run(): void }>) {
  m.get("a")!.run();
}
namespace N {
  export type T = { m(): void };
}
export function inNamespace(t: N.T) {
  t.m();
}
class K {
  ops: { run(): void } = { run() {} };
}
export function classProperty(k: K) {
  k.ops.run();
}
type Tag = { tag(s: TemplateStringsArray): void };
export function taggedTemplate(t: Tag) {
  t.tag`x`;
}

// ---- an accessor signature
type Getter = { get v(): number };
export function readsTypeGetter(g: Getter) {
  g.v;
}
type Setter = { set v(x: number) };
export function writesTypeSetter(g: Setter) {
  g.v = 1;
}
interface IGetter {
  get v(): number;
}
export function readsInterfaceGetter(g: IGetter) {
  g.v;
}

// ---- an interface's method signature, and an abstract method
interface Writer {
  write(s: string): void;
}
export function viaInterface(w: Writer) {
  w.write("x");
}
interface OverloadedWriter {
  m(x: string): void;
  m(x: number): void;
}
export function interfaceOverloads(o: OverloadedWriter) {
  o.m(1);
}
abstract class Base {
  abstract m(): void;
}
export function abstractMethod(b: Base) {
  b.m();
}
declare global {
  interface Window {
    foo(): void;
  }
}
export function declaredGlobal() {
  window.foo();
}
class JsonWriter {
  write(s: string) {}
}
export function classReceiver(w: JsonWriter) {
  w.write("x");
}
export function unionWithInterface(w: JsonWriter | Writer) {
  w.write("x");
}

// ---- the neighbours, which have no signature of a method
type CallSignature = { (x: string): void };
export function callSignature(f: CallSignature) {
  f("a");
}
interface ICallSignature {
  (x: string): void;
}
export function interfaceCallSignature(f: ICallSignature) {
  f("a");
}
type ConstructSignature = { new (): object };
export function constructSignature(c: ConstructSignature) {
  new c();
}
interface IConstructSignature {
  new (): object;
}
export function interfaceConstructSignature(c: IConstructSignature) {
  new c();
}
type FunctionType = (x: string) => void;
export function functionType(f: FunctionType) {
  f("a");
}
type Factory = { Make: new () => object };
export function constructorMember(t: Factory) {
  new t.Make();
}
declare class Declared {
  m(): void;
}
export function declaredClass(d: Declared) {
  d.m();
}
type Indexed = { [k: string]: () => void };
export function indexSignature(r: Indexed) {
  r.x();
}
export function record(r: Record<string, () => void>) {
  r.x();
}
