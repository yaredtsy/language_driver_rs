def adder(n: U32) -> U32 -> U32:
  x => x

def main() -> U32:
  add2 = adder(2)
  adder(2)(add2(1))
