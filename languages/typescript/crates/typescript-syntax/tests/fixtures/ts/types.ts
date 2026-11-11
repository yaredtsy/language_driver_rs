interface Writer {
  write(chunk: string): void;
  close(): void;
  readonly name: string;
  onData: (x: number) => void;
  (call: number): string;
  new (x: number): Writer;
}

interface Overloaded {
  parse(a: string): number;
  parse(a: number): number;
}

export interface Exported<T> extends Writer {
  extra<U>(u: U): T;
}

enum Color {
  Red,
  Green = 2,
}

const enum Flag {
  A = 1,
}

type Alias = string | number;

type ObjectType = {
  method(): void;
};

export type { Alias as Renamed };

namespace Plain {
  export function inside() {}
  export const arrow = () => {};
  export class InNs {}
  export namespace Deeper {
    export function deepest() {}
  }
}

namespace A.B.C {
  export function viaDots() {}
}

declare module "external-package" {
  export function fromModule(): void;
}

declare global {
  interface Window {
    custom(): void;
  }
}

module Legacy {
  export function oldStyle() {}
}
