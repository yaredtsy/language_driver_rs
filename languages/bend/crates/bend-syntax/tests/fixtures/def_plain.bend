# Adds two numbers.
# Second line of the doc.
def add(a: Nat, b: Nat) -> Nat:
  match a:
    case Zero{}: b
    case Succ{p}: Succ{add(p, b)}

# a blank line follows this comment, so it is not the doc of main

def main() -> Nat: add(1n, 2n)
def Nat.double(n: Nat) -> Nat: add(n, n)
