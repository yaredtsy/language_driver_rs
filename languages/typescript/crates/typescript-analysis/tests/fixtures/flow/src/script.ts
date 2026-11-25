function sa() {}
let global = sa;
declare function later(cb: () => void): void;
later(() => {
  let local = sa;
  local();
});
