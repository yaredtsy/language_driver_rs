# file header, a blank line follows

# doc of f, first line
# doc of f, second line
def f() -> Nat: 0 # trailing comment on the declaration's line
# a comment between declarations, a blank line follows

# doc of g
@unsafe def g() -> Nat:
  # inside the body
  1 # after code
  # last comment of the body, indented

def h() -> Nat: "#" # a hash in a string is not a comment
def i() -> Nat: '#'
def j() -> Nat: '\''
#| expected output
#| second line
