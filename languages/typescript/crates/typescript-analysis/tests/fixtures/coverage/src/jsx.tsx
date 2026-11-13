declare global {
  namespace JSX {
    interface Element {}
    interface IntrinsicElements {
      [name: string]: any;
    }
  }
}

declare function Comp(props: { n: number }): JSX.Element;
declare function Wrap(props: { children?: unknown }): JSX.Element;

export function Page() {
  return (
    <div>
      <Comp n={1} />
      <Wrap>text</Wrap>
      <my-el />
      <svg:rect />
    </div>
  );
}

export const top = <Comp n={2} />;
