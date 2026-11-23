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

def main(b: Bool) -> U32:
  pick(b)(1)
