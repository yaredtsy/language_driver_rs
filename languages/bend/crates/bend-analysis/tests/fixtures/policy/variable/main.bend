def main(f: U32 -> U32, xs: List(U32 -> U32)) -> U32:
  g = f
  match xs:
    case Con{k, rest}:
      k(g(f(1)))
    case Nil{}:
      0
