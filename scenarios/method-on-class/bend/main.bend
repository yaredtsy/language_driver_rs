import Base

# Bend has no methods: `Counter.bump` is one name that happens to have a dot, and `Counter.bump(c)` is a call to it like any other.
type Counter is Data:
  C{n: U32}

def Counter.read(c: Counter) -> U32:
  1

def Counter.bump(c: Counter) -> U32:
  Counter.read(c)

def Counter.reset() -> U32:
  0

def main(c: Counter) -> U32:
  U32.add(Counter.bump(c), Counter.reset())
