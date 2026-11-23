def a() -> Nat:
  (x = 1n
   x)

def b(x: Nat) -> Nat:
  (match x:
    case Zero{}: 0n
    case Succ{p}: p)

def c() -> Pair: (a, b, c)
def d() -> Pair: ((a, b), (c))
