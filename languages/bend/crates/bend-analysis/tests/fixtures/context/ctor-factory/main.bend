type Game is Data:
  Game{step: U32 -> U32, read: U32 -> U32}

def step(x: U32) -> U32:
  x

def read(x: U32) -> U32:
  x

def game() -> Game:
  Game{step, read}

def play(g: Game) -> U32:
  Game{s, r} = g
  s(1)

def main() -> U32:
  play(game())
