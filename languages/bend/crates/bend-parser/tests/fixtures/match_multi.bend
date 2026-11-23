def a(x: Nat, y: Nat) -> Nat:
  match x y:
    case Zero{} Zero{}: 0n
    case Succ{p} q: p
    case _ _: 1n

def b(x: Nat, y: Nat) -> Nat:
  match x, y:
    case Zero{}, Zero{}: 0n
    case a, b: a

def c(a: Nat, b: Nat) -> Nat:
  match a b:
    case K{x, _} 1n+p: x
    case _ 0n: 1n
