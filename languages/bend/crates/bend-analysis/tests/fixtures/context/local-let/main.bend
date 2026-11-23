def work(x: U32) -> U32:
  x

def each(f: U32 -> U32, x: U32) -> U32:
  f(x)

def main() -> U32:
  g = work
  each(g, 1)
