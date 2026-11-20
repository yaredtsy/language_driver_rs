law good:
  for x: Nat
  Eq(Nat, x, x)

law bad
  for x: Nat

law after:
  Eq(Nat, 1n, 1n)
