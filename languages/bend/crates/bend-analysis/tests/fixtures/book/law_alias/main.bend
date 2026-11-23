import Base
import ./code.bend as C
import ./LAWS.bend as Laws

def Laws.sorted(xs):
  match xs:
    case Nil{}:
      Unit{}
    case h <> t:
      Unit{}

def use(xs: List<&2, Nat>) -> Data:
  Laws.sorted(xs)
