def leaf(x: U32) -> IO(Unit):
  import "./leaf.c"

def main() -> IO(Unit):
  leaf(1)
