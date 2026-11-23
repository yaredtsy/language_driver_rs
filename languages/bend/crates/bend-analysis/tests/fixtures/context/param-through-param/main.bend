def work(x: U32) -> U32:
  x

def each(f: U32 -> U32, x: U32) -> U32:
  f(x)

def apply(h: (U32 -> U32) -> U32 -> U32, f: U32 -> U32, x: U32) -> U32:
  h(f, x)

def main() -> U32:
  apply(each, work, 1)
