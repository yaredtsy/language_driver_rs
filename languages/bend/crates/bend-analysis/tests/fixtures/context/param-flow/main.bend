def work(x: U32) -> U32:
  x

def each(f: U32 -> U32, x: U32) -> U32:
  f(x)

def relay(f: U32 -> U32, x: U32) -> U32:
  each(f, x)

def main() -> U32:
  relay(work, 1)
