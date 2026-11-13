declare function tag(strings: TemplateStringsArray, ...values: number[]): string;
declare function f(): number;

export function use() {
  return tag`a${f()}b`;
}

export const top = tag`x`;
export const raw = String.raw`y`;
