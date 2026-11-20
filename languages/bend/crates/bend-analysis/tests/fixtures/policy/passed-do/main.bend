type Parser is Data:
  P{run: U32}

def Parser.bind(m: Parser, k: U32) -> Parser:
  m

def Parser.pure(x: U32) -> Parser:
  P{x}

def name() -> U32:
  1

def pop() -> U32:
  2

def tick() -> U32:
  3

def main() -> Parser:
  do Parser<U32>:
    x : U32 <- name
    Unit <- pop
    tick
    return x
