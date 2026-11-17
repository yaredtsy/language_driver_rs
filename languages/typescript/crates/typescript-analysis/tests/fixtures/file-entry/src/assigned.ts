function g() {}

const X: { y?: () => void } = {};

X.y = () => {
  g();
};
