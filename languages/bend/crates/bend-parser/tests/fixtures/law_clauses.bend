law sorted_insert:
  for ~k: Nat
  for -A: Data
  for xs: List<Nat> where Sorted(xs)
  exs ys: List<Nat> where Sorted(ys)
  Eq(Nat, 1n, 1n)

law plain:
  Eq(Nat, 1n, 1n)
