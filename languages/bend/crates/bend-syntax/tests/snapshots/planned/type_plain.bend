# ID: FILE

# A list.
# ID: d0
type List<a, -A: Kind(a)> is Kind(a):
  Nil{}
  Con{head: A, tail: List<a, A>}

# ID: d1
type Unit is Data:
  Unit{}
