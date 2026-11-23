import Base

# `each` calls whatever `f` its caller hands it: a def by name, or a lambda written at the call.
def work(x: U32) -> U32:
  x

def each(f: U32 -> U32, x: U32) -> U32:
  f(x)

def main() -> U32:
  a = each(work, 1)
  each(x => work(x), a)
