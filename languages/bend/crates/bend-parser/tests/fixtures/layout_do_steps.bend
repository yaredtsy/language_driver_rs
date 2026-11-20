def main() -> IO(Unit):
  do IO<Unit>:
    IO.print("a")
    IO.print("b");
    n : U32 <- IO.read()
    Unit <- IO.print(U32.show(n))
    m : U32 = 3
    return m

def last() -> IO(Nat):
  do IO<Nat>:
    IO.pure(1n)
