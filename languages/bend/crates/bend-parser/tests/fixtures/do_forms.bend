def a() -> IO(Unit):
  do IO<Unit>:
    x : Nat <- IO.read()
    y : Nat = 2n
    IO.print(show(x))
    Unit <- IO.print(show(y))
    return Unit{}

def b() -> Parser(Nat):
  do Parser<Nat>:
    c : Char <- item
    return 1n

def c() -> M(A):
  do M<>:
    x : A <- m
    x
def d() -> M(A):
  do M<L, R>:
    return v
