def work(x: U32) -> U32:
  x

def id(f: U32 -> U32) -> U32 -> U32:
  f

def main() -> U32:
  id(work)(1)
