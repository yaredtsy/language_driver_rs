import Base

def main() -> IO(Unit):
  do IO<Unit>:
    IO.print("a")
    return Unit{}

def maybe(x: U32) -> Maybe<U32>:
  do Maybe<U32>:
    y : U32 <- Some{x}
    return y
