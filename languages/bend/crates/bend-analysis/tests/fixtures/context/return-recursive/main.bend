def work(x: U32) -> U32:
  x

def loop(n: Nat) -> U32 -> U32:
  match n:
    case Zero{}:
      work
    case Succ{p}:
      loop(p)

def main() -> U32:
  loop(3n)(1)
