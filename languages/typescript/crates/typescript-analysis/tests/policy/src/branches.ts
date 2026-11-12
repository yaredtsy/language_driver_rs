function a() {}
function b() {}
function c() {}
function d() {}
function e() {}
// every call is a site, whatever branch it is in
export function branches(flag: boolean, n: number) {
  if (flag) {
    a();
  } else {
    b();
  }
  switch (n) {
    case 1:
      c();
      break;
    default:
      d();
  }
  flag ? a() : b();
  flag && c();
  for (let i = 0; i < n; i++) {
    d();
  }
  try {
    e();
  } catch {
    a();
  } finally {
    b();
  }
}
