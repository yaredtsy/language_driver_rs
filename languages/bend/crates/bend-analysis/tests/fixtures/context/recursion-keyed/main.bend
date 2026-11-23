def work(x: U32) -> U32:
  x

def f(~g: U32 -> U32, n: U32) -> U32:
  g(f(~g, n))

def r(~g: U32 -> U32, n: U32) -> U32:
  r(~(y => g(y)), n)

def main() -> U32:
  a = f(~work, 1)
  r(~work, 1)
