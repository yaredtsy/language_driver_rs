type Game is Data:
  Game{step: U32 -> U32, read: U32 -> U32}

def work(x: U32) -> U32:
  x

def other(x: U32) -> U32:
  x

def add(a: U32, b: U32) -> U32:
  a

def pick(b: Bool) -> U32 -> U32:
  match b:
    case True{}:
      work
    case False{}:
      other

def each(f: U32 -> U32, x: U32) -> U32:
  f(x)

def play(g: Game) -> U32:
  Game{s, r} = g
  s(1)

def main(b: Bool) -> U32:
  p = each(pick(b), 1)
  q = each(add(1), 5)
  play(Game{work, other})
