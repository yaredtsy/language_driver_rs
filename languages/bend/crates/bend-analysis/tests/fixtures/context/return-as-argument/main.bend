def work(x: U32) -> U32:
  x

def pick() -> U32 -> U32:
  work

def each(f: U32 -> U32, x: U32) -> U32:
  f(x)

def main() -> U32:
  each(pick(), 1)
