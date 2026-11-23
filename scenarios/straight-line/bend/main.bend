def store(a: U32, b: U32) -> U32:
  a

def parse() -> U32:
  1

def fetch() -> U32:
  parse()

def transform() -> U32:
  2

def main() -> U32:
  a = fetch()
  b = transform()
  store(a, b)
