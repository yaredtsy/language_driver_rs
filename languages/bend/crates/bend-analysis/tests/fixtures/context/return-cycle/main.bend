def work(x: U32) -> U32:
  x

def other(x: U32) -> U32:
  x

def a(n: Nat) -> U32 -> U32:
  match n:
    case Zero{}:
      work
    case Succ{p}:
      b(p)

def b(n: Nat) -> U32 -> U32:
  match n:
    case Zero{}:
      other
    case Succ{p}:
      a(p)

def from_a() -> U32:
  a(2n)(1)

def from_b() -> U32:
  b(2n)(1)
