def work(x: U32) -> U32:
  x

def g(f: U32 -> U32) -> U32:
  f(1)

def each(f: U32 -> U32, x: U32) -> U32:
  f(x)

def h(k: U32 -> U32) -> U32:
  1

def main() -> U32:
  a = h(g)
  each(work, 1)
