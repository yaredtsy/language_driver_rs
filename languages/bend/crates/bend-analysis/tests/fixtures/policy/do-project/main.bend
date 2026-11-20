type Parser is Data:
  P{run: U32}

def Parser.bind(m: Parser, k: U32) -> Parser:
  m

def Parser.pure(x: U32) -> Parser:
  P{x}

def item() -> Parser:
  P{1}

def main() -> Parser:
  do Parser<U32>:
    x : U32 <- item()
    return x
