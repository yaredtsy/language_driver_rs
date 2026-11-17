/** ID: SAME */
function a() {
  x();
}

/** ID: SAME */
function b() {
  y();
}

function x() {}
function y() {}

export function main() {
  a();
  b();
}

export function twice() {
  a();
  b();
  b();
}

/** ID: TRI */
function p() {
  x();
}

/** ID: TRI */
function q() {
  x();
}

/** ID: TRI */
function r() {
  x();
}

export function three() {
  p();
  q();
  r();
}

/** ID: KF */
class K {
  m() {}
}

/** ID: KF */
function f() {
  x();
}

export function mixed() {
  new K();
  f();
}

/** ID: UP */
function copy() {
  x();
}

/** ID: UP */
function original() {
  y();
}

export function above() {
  original();
  copy();
}
