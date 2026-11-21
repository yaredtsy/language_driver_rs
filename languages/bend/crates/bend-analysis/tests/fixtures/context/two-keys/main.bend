def work(x: U32) -> U32:
  x

def other(x: U32) -> U32:
  x

def each(f: U32 -> U32, x: U32) -> U32:
  f(x)

def main() -> U32:
  a = each(work, 1)
  b = each(other, 2)
  each(work, 3)
