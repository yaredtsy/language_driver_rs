def helper() -> U32:
  1

def countdown(n: Nat) -> U32:
  match n:
    case 1n+p:
      countdown(p)
    case 0n:
      helper()

def main() -> U32:
  countdown(3n)
