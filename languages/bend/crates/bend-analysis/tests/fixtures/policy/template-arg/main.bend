def twice(~h: U32 -> U32, x: U32) -> U32:
  h(h(x))

def f(x: U32) -> U32:
  x

def g(x: U32) -> U32:
  x

def main() -> U32:
  a = twice(~(x => f(x)), 40)
  twice(~g, a)
