import Base

def each(f: U32 -> U32, x: U32) -> U32:
  f(x)

def main() -> U32:
  each(U32.show, 1)
