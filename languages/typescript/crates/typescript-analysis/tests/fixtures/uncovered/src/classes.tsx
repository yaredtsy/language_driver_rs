// Sites that sit in a class's own body, which a construction does not draw (the class's instance field initializers), and the shapes around them.
function side() {}
function make(f?: () => void) {}
function inner() {}
declare function dec(target: Function): void;
declare function factory(n: number): (target: Function) => void;
declare function memberDec(target: object, key: string | symbol): void;

// ---- 3: a construction of a class whose instance field initializers hold sites. One for each site, at each construction.
export class Plain {
  x = 1;
}
export class WithField {
  x = side();
}
export class WithTwoFields {
  x = side();
  y = make(() => inner());
}
// an arrow held by a field is its own def: its calls are its own, not the class body's
export class WithArrowField {
  onClick = () => side();
}
// a call inside an anonymous function that is only part of an initializer stays with the class body
export class WithWrappedArrow {
  x = make(() => inner());
}
export class WithConstructor {
  x = side();
  constructor() {
    side();
  }
}
export class StaticOnly {
  static s = side();
  static {
    side();
  }
}
export class ParameterProperty {
  constructor(private p = side()) {}
}
export class Base {
  b = side();
}
export class DerivedNoConstructor extends Base {
  d = side();
}
export class DerivedConstructor extends Base {
  d = side();
  constructor() {
    super();
  }
}
export class Grandchild extends DerivedNoConstructor {}
// the initializer of an instance field and a site that is not in one (a static initializer): only the first is what a construction runs
export class Mixed {
  x = side();
  static s = side();
}

// 0
export function constructsPlain() {
  new Plain();
}
// 1
export function constructsWithField() {
  new WithField();
}
// 2: one for each construction
export function constructsTwice() {
  new WithField();
  new WithField();
}
// side(), make(), inner(): 3
export function constructsWithTwoFields() {
  new WithTwoFields();
}
// the arrow field is a def: 0
export function constructsArrowField() {
  new WithArrowField();
}
// make(), inner(): 2
export function constructsWrappedArrow() {
  new WithWrappedArrow();
}
// the class has a constructor and the field initializer is still not in it: 1
export function constructsWithConstructor() {
  new WithConstructor();
}
// static initializers and blocks run when the class is defined: 0
export function constructsStaticOnly() {
  new StaticOnly();
}
// a parameter's default is a site of the constructor, which is drawn: 0
export function constructsParameterProperty() {
  new ParameterProperty();
}
// no constructor: the field initializers of the class and of the base class that runs for it: 1 + 1
export function constructsDerivedNoConstructor() {
  new DerivedNoConstructor();
}
// the derived class's own constructor calls `super()`: that construction of `Base` is counted where it is written (the constructor's body): 1 + 1
export function constructsDerivedConstructor() {
  new DerivedConstructor();
}
// three classes on the chain, none with a constructor: Grandchild has none, DerivedNoConstructor has none, Base has none, so 0 + 1 + 1
export function constructsGrandchild() {
  new Grandchild();
}
// 1, not 2
export function constructsMixed() {
  new Mixed();
}
// a class that is not the project's: nothing to look into: 0
export function constructsExternal() {
  new Map();
}

// ---- 4: a decorated class declared inside a function: its decorators are in no tree of the function. One for each site of a decorator.
export function declaresDecorated() {
  @dec
  class Local {}
}
export function declaresTwo() {
  @dec
  class A {}
  @dec
  class B {}
}
export function declaresDecoratedMember() {
  class Local {
    @memberDec m() {}
  }
}
// `@factory(1)` is two sites: the call of `factory` and the call of its result
export function declaresFactoryDecorated() {
  @factory(1)
  class Local {}
}
// the class's other sites are not decorators: 1
export function declaresDecoratedWithOtherSites() {
  @dec
  class Local {
    static s = side();
  }
}
export function declaresPlain() {
  class Local {
    m() {}
  }
}
// a decorated class at the top level of the file is not inside a function: the control
@dec
export class TopDecorated {}
export function usesTopDecorated() {
  new TopDecorated();
}

// ---- 5: a class component: `<Cls />` constructs it, and React calls its `render`, which is not drawn
export class Widget {
  props = {};
  render() {
    return <div />;
  }
}
class BaseWidget {
  props = {};
  render() {
    return <div />;
  }
}
export class DerivedWidget extends BaseWidget {}
export class ArrowWidget {
  props = {};
  render = () => <div />;
}
// `render` is declared by a package's `.d.ts`: nothing of the project is hidden: 0
export class FromLibrary extends LibraryComponent {}
function FunctionComponent(props: {}) {
  return <div />;
}
// 1
export function showsWidget() {
  return <Widget />;
}
// `render` is the base class's, a def of the project: 1
export function showsDerivedWidget() {
  return <DerivedWidget />;
}
// `render` is an arrow held by a field: a def of the project: 1
export function showsArrowWidget() {
  return <ArrowWidget />;
}
// 2: one for each element
export function showsTwice() {
  return (
    <>
      <Widget />
      <Widget />
    </>
  );
}
export function showsLibraryComponent() {
  return <FromLibrary />;
}
// a function component is called, and what it calls is drawn: 0
export function showsFunctionComponent() {
  return <FunctionComponent />;
}
// `new Widget()` constructs it and nothing calls `render`: 0
export function constructsWidget() {
  new Widget();
}
