# A type with a marker. Bend has no constructor function, so building it runs no code of the program's: a leaf.
#
# ID: 11111111-0000-0000-0000-000000000001
type Report is Data:
  R{n: U32}

# ID: 11111111-0000-0000-0000-000000000002
type Plain is Data:
  P{n: U32}

def main() -> Plain:
  a = R{1}
  P{2}
