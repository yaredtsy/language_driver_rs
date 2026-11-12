function leaf() {}
export function over(x: string): void;
export function over(x: number): void;
export function over(x: string | number) {
  leaf();
}
export function usesOverload() {
  over("s");
}
