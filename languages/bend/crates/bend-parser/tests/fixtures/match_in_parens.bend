def a(x: Nat) -> Nat:
  f((match x:
       case Zero{}: 0n
       case Succ{p}: p), y)
