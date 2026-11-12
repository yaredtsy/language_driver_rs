function tag(strings: TemplateStringsArray, ...values: unknown[]) {
  helper();
}
function helper() {}
interface Formatter {
  fmt(strings: TemplateStringsArray): string;
}
declare const formatter: Formatter;
declare function factory(): (strings: TemplateStringsArray) => string;

// the tag is called, and so is a call in a substitution; a method as the tag; a tag that is a call's result (the call itself is a site, and its result is no
// declaration); a library function
export function tagged() {
  tag`a ${helper()} b`;
  formatter.fmt`x`;
  factory()`y`;
  String.raw`z`;
}
