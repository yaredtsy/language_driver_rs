function plain(target: Function) {}
function onMethod(target: object, key: string | symbol, descriptor: PropertyDescriptor) {}
function onParameter(target: object, key: string | symbol | undefined, index: number) {}
function inlineFactory(n: number) {
  return (target: object, key: string | symbol) => {
    helper(n);
  };
}
function helper(n: number) {}
function applyNamed(target: object, key: string | symbol, descriptor: PropertyDescriptor) {}
function namedFactory(n: number) {
  return applyNamed;
}
function parameterFactory(label: string) {
  return onParameter;
}
declare function declared(): ClassDecorator;

// the decorators of a class, of its members and of the parameters of its constructor and methods are the class's own body
@plain
@declared()
export class Decorated {
  @inlineFactory(1) field = 0;
  @onMethod
  @namedFactory(2)
  method(@parameterFactory("x") p: string) {}
  constructor(@onParameter private readonly dep: string) {}
}
