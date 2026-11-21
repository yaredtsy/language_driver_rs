def work(x: U32) -> U32:
  x

def app(f: U32 -> U32, x: U32) -> U32:
  f(x)

def main() -> U32:
  p = app(work)
  p(1)
