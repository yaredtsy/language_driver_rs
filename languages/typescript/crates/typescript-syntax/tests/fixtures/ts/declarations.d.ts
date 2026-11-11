export declare function overloaded(a: string): string;
export declare function overloaded(a: number): number;

export declare class Widget {
  constructor(x: number);
  render(): void;
  render(flag: boolean): void;
  static create(): Widget;
}

declare const VERSION: string;

declare function noImplementation(): void;
