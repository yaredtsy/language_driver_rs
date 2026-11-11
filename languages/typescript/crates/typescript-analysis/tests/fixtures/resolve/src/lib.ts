/**
 * Returns one.
 * ID: PLAIN-0001
 */
export function plain(): number {
  return 1;
}

export class Counter {
  count = 0;
  bump(): number {
    return ++this.count;
  }
  static make(): Counter {
    return new Counter();
  }
}

export interface Writer {
  write(text: string): void;
}

export class JsonWriter implements Writer {
  write(text: string): void {}
}

export class CsvWriter implements Writer {
  write(text: string): void {}
}

export function overloaded(x: string): string;
export function overloaded(x: number): number;
export function overloaded(x: any): any {
  return x;
}

export function generic<T>(x: T): T {
  return x;
}

export default function fallback(): void {}

export const arrow = (): number => 2;

export * from "./more";
