def work(x: U32) -> U32:
  x

def other(x: U32) -> U32:
  x

def pick(b: Bool) -> U32 -> U32:
  match b:
    case True{}:
      work
    case False{}:
      other

def each(f: U32 -> U32, x: U32) -> U32:
  f(x)

def main(b: Bool) -> U32:
  each(pick(b), 1)
