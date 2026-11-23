# ID: FILE

# Adds two numbers.
# Second line of the doc.
# ID: d0
def add(a: Nat, b: Nat) -> Nat:
  match a:
    case Zero{}: b
    case Succ{p}: Succ{add(p, b)}

# a blank line follows this comment, so it is not the doc of main

# ID: d1
def main() -> Nat: add(1n, 2n)
# ID: d2
def Nat.double(n: Nat) -> Nat: add(n, n)
