import Base

def IsEven(n: Nat) -> Data:
  Unit

def main(n: Nat, xs: List<Nat>) -> Or(IsEven(n), Empty):
  x = {Unit{} : IsEven(n)}
  x
