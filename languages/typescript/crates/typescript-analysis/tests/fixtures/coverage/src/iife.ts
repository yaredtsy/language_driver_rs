declare function f(): number;
declare function g(callback: () => unknown): void;

export function run() {
  (() => {
    f();
  })();
  g(function () {
    f();
  });
  const o = {
    m() {
      f();
    },
  };
  return o;
}

(function () {
  f();
})();
g(() => f());
