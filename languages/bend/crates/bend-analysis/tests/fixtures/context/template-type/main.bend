def sky(x: U32) -> U32:
  x

def blk(~S: Data, ~at: S -> U32, s: S) -> U32:
  at(s)

def main() -> U32:
  blk(~U32, ~sky, 1)
