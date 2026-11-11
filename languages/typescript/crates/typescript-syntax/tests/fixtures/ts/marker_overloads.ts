/**
 * Overloaded.
 * ID: overloads-1
 */
function marked(a: string): string;
function marked(a: number): number;
function marked(a: any): any {
  return a;
}

/**
 * Overloaded and unmarked.
 */
function unmarked(a: string): string;
function unmarked(a: number): number;
function unmarked(a: any): any {
  return a;
}

/** Only the implementation is documented. */
function implementationDoc(a: string): string;
function implementationDoc(a: number): number;
/** ID: impl-doc-ignored */
function implementationDoc(a: any): any {
  return a;
}
