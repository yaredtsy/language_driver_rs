def g(x: U32) -> U32:
  x

def each(f: U32 -> U32, x: U32) -> U32:
  f(x)

def outer(~h: U32 -> U32, x: U32) -> U32:
  each(y => h(y), x)

def main() -> U32:
  outer(~g, 1)
