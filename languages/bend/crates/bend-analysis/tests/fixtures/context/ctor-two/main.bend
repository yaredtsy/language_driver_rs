type Sum is Data:
  L{f: U32 -> U32}
  R{f: U32 -> U32}

def work(x: U32) -> U32:
  x

def main() -> U32:
  x = L{work}
  match x:
    case L{f}:
      f(1)
    case R{f}:
      f(2)
