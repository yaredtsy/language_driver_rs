def twice(~h: U32 -> U32, x: U32) -> U32:
  h(h(x))

def g(x: U32) -> U32:
  x

def main() -> U32:
  twice(~g, 40)
