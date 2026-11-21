type Box is Data:
  Box{f: U32 -> U32}

def work(x: U32) -> U32:
  x

def other(x: U32) -> U32:
  x

def pick(b: Bool) -> Box:
  match b:
    case True{}:
      Box{work}
    case False{}:
      Box{other}

def main(b: Bool) -> U32:
  Box{f} = pick(b)
  f(1)
