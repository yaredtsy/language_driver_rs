# ID: FILE

# the claim
# ID: d0
law add_zero:
  for n: Nat
  Eq(Nat, add(n, 0n), n)

# the proof
# ID: d1
def add_zero(n):
  match n:
    case Zero{}: {==}
    case Succ{p}: {==}

# ID: d2
law clauses:
  for ~k: Nat
  for -A: Data
  for +x: A where P(x)
  exs y: A
  Eq(A, y, y)
