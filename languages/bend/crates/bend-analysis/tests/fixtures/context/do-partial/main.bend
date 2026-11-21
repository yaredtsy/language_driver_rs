import Base

def P(-A: Type) -> Type:
  (A -> U32 -> U32) -> U32 -> U32

def P.pure(-A: Type, x: A) -> P(A):
  k => s => k(x, s)

def P.bind(-A: Type, -B: Type, m: P(A), f: A -> P(B)) -> P(B):
  k => s => m(a => f(a)(k), s)

def drop(n: Nat, k: Unit -> U32 -> U32, s: U32) -> U32:
  k(Unit{}, s)

def main() -> P(Unit):
  do P<Unit>:
    Unit <- drop(2n)
    return Unit{}
