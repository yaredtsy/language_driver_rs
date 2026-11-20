# ID: FILE

# ID: d0
def outer(x: Nat) -> Nat:
  inner = y => add(y, x)
  match x:
    case Zero{}: inner(0n)
    case Succ{p}: (z => z)(p)

# ID: d1
def with_do() -> IO(Unit):
  do IO<Unit>:
    x : Nat = 1n
    return Unit{}
