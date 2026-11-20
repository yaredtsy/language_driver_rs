law add_zero:
  for n: Nat
  Eq(Nat, add(n, 0n), n)

def add_zero(n):
  match n:
    case Zero{}: {==}
    case Succ{p}: {==}

def Laws.sorted(xs): xs
