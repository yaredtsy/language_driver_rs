def add(a: U32, b: U32) -> U32:
  a

def each(f: U32 -> U32, x: U32) -> U32:
  f(x)

def main() -> U32:
  each(add(1), 5)
