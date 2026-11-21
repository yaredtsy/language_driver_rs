def add(x: U32, y: U32) -> U32:
  x

def adder(k: U32) -> U32 -> U32:
  x => add(x, k)

def main() -> U32:
  add2 = adder(2)
  add2(1)
