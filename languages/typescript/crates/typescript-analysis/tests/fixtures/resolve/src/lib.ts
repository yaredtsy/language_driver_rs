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

export function second(): number {
  return 2;
}

export function generic<T>(x: T): T {
  return x;
}

export default function fallback(): void {}

export const arrow = (): number => 2;

export class ArrowA {
  write = (text: string): void => {};
}

export class ArrowB {
  write = (text: string): void => {};
}

export class Child extends Counter {}

export const api = {
  load(): number {
    return 1;
  },
};

export interface Reader {
  read(a: string): void;
}

export interface Reader {
  read(a: number): void;
}

export class Greeter {
  helper(): number {
    return 1;
  }
  run(): number {
    return this.helper();
  }
}

export function makeHandler(): () => void {
  return () => {};
}

export class Base2 {
  constructor(public x: number) {}
}

export class Derived2 extends Base2 {
  constructor() {
    super(1);
  }
}

export class NoConstructor {}

export class FromNoConstructor extends NoConstructor {
  constructor() {
    super();
  }
}

export * from "./more";
