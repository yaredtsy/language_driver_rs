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
// a named function expression is a def, and its call is still unresolved (see file_entry.rs)
(function named() {
  h();
})();
