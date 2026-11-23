def work(x: U32) -> U32:
  x

def pick() -> U32 -> U32:
  work

def make() -> U32 -> U32:
  pick()

def main() -> U32:
  make()(1)
