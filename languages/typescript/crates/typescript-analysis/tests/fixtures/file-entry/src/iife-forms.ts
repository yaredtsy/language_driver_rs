function h() {}

// the call of a function expression written in place: nothing about it is unknown, and no declaration is to be entered
(() => {
  h();
})();
(function () {
  h();
})();
(async () => {
  h();
})();
!(function () {
  h();
})();
// a named function expression is a def, and its call where it is written is a Call to that def (H11)
(function named() {
  h();
})();
export function inside() {
  (function n1() {
    h();
  })();
}
// under a type assertion, `as`, and `void`: the same def
(<any>function n2() {
  h();
})();
(function n3() {
  h();
} as () => void)();
void (function n5() {
  h();
})();
