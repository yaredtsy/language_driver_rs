def f(a: Nat, b: Nat) -> Nat:
  match a:
    case Zero{}:
      match b:
        case Zero{}: 0n
        case Succ{q}: q
    case Succ{p}: p

def g(a: Nat) -> Nat:
  match a:
  case Zero{}: 0n
