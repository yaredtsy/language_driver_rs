import Base

def work(x: U32) -> U32:
  x

def main() -> U32:
  m = Some{work}
  match m:
    case Some{f}:
      f(1)
