def a() -> Nat:
  x = 1

  y = 2   # after a blank line, still the body

# a column-0 comment inside a body does not end it
  x
def b() -> String: "multi
line string
with column-0 text"
def c() -> Nat: 0
def last() -> Nat: 9
