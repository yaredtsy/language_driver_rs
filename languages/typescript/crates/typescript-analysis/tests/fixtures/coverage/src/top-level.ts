declare function f(): number;
declare function g(a?: number): number;

export const x = f();
export default g(f());
f();
export const d = new Date();
