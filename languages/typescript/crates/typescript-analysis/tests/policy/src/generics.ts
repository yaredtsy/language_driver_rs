function leaf() {}
function id<T>(x: T): T {
  leaf();
  return x;
}
export function usesGeneric() {
  id<number>(1);
}
