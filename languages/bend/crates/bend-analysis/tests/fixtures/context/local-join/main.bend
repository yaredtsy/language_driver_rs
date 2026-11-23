def work(x: U32) -> U32:
  x

def other(x: U32) -> U32:
  x

def main(b: Bool) -> U32:
  g = (match b:
         case True{}:
           work
         case False{}:
           other)
  g(1)
