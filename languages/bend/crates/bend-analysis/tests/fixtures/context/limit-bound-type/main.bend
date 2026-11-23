type Vec2 is Data:
  V{x: U32}

def g(-T: Data, a: T, b: T) -> T:
  (a + b : T)

def main() -> Vec2:
  p = V{1}
  q = V{2}
  g(Vec2, p, q)
