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
declare class Foreign {
  get callback(): () => void;
}
declare class LibraryComponent {
  props: {};
  render(): unknown;
}
declare const holder: { get callback(): () => void };
