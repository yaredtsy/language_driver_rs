def work(x: U32) -> U32:
  x

def run(f: U32 -> U32, x: U32) -> U32:
  g = f
  g(x)

def main() -> U32:
  run(work, 1)
