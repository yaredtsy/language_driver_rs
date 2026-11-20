import Base
import ./code.bend as C

law sorted:
  for +xs: List<&2, Nat>
  C.is_sorted(xs)

law lonely:
  for +xs: List<&2, Nat>
  C.is_sorted(xs)
