declare global {
  namespace JSX {
    interface Element {}
    interface ElementClass {
      render(): unknown;
    }
    interface ElementChildrenAttribute {
      children: {};
    }
    interface IntrinsicElements {
      [name: string]: any;
    }
  }
}

declare function memo<P>(component: (props: P) => JSX.Element): (props: P) => JSX.Element;

function Plain(props: { a?: number; children?: unknown }) {
  return <div />;
}
const Arrow = (props: { children?: unknown }) => <div />;
class ClassComponent {
  props = {};
  render() {
    return <div />;
  }
}
const Inner = (props: {}) => <div />;
const Memoized = memo(Inner);
declare const Ns: { Icon: (props: {}) => JSX.Element };

// an intrinsic tag, a custom element and a namespaced name are no site; nothing is called
export function intrinsic() {
  return (
    <div>
      <my-el />
      <svg:rect />
    </div>
  );
}

// one edge for each component, a function, an arrow, a class (a construction), a memoized one and a property of a declared object (both of the last two are
// values: function_typed_value). `<Arrow></Arrow>` has an opening and a closing tag and is one site
export function components() {
  return (
    <div>
      <Plain a={1} />
      <Arrow></Arrow>
      <ClassComponent />
      <Memoized />
      <Ns.Icon />
    </div>
  );
}

// a closing tag is not a second site
export function closing() {
  return <Plain>text</Plain>;
}

// a property of a parameter
export function fromProps(props: { Icon: (p: {}) => JSX.Element }) {
  return <props.Icon />;
}
