import React from "react";

function Greeting({ name }: { name: string }) {
  return <h1>Hello, {name}</h1>;
}

const Arrow = (props: { items: string[] }) => (
  <ul>
    {props.items.map((item) => (
      <li key={item}>{item}</li>
    ))}
  </ul>
);

class Counter extends React.Component<{}, { n: number }> {
  state = { n: 0 };
  increment = () => this.setState({ n: this.state.n + 1 });
  render() {
    return <button onClick={this.increment}>{this.state.n}</button>;
  }
}

function generic<T,>(value: T) {
  const wrapped = <T,>(x: T) => x;
  return <div>{String(wrapped(value))}</div>;
}

export default function App() {
  const onClick = () => {};
  return (
    <>
      <Greeting name="a" />
      <Arrow items={["x"]} />
    </>
  );
}
