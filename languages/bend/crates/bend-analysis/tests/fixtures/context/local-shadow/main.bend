def work(x: U32) -> U32:
  x

def other(x: U32) -> U32:
  x

def run(f: U32 -> U32, x: U32) -> U32:
  f = other
  f(x)

def main() -> U32:
  run(work, 1)
