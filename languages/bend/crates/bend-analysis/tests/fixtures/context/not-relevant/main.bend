def work(x: U32) -> U32:
  x

def other(x: U32) -> U32:
  x

def log(f: U32 -> U32, x: U32) -> U32:
  x

def main() -> U32:
  a = log(work, 1)
  log(other, 2)
