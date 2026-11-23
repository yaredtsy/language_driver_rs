import Base

def view(s: U32) -> U32:
  s

def tick(s: U32) -> U32:
  s

def g(x: U32) -> U32:
  x

def h(f: U32 -> U32) -> U32:
  f(1)

def main() -> IO(Unit):
  r = h(g)
  App.run(~U32, ~App{view, tick})
