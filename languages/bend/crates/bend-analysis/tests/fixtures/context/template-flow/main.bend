def g(x: U32) -> U32:
  x

def twice(~h: U32 -> U32, x: U32) -> U32:
  h(h(x))

def middle(~h: U32 -> U32, x: U32) -> U32:
  twice(~h, x)

def outer(~h: U32 -> U32, x: U32) -> U32:
  middle(~h, x)

def main() -> U32:
  outer(~g, 1)
