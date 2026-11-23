import Base

def main(x: U32, h: U32, t: List(U32), p: Nat) -> U32:
  a = Some{x}
  b = Con{h, t}
  c = [1, 2]
  d = "s"
  e = 3n+p
  (a, b)
