type App is Data:
  App{v: U32 -> U32, t: U32 -> U32}

def work(x: U32) -> U32:
  x

def other(x: U32) -> U32:
  x

def main() -> U32:
  App{v, t} = App{work, other}
  t(1)
