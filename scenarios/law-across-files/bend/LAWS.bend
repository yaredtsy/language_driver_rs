# The law, stated where the proof's file and the code's file can both see it.
import Base

law sorted:
  for xs: List<Nat>
  {xs == xs : List<Nat>}
