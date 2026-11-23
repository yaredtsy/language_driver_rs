import Base
import ./LAWS.bend as Laws
import ./PROOF.bend as Proof

def main(xs: List<Nat>) -> Unit:
  x = Laws.sorted(xs)
  Unit{}
