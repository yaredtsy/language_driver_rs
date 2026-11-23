import Base
import ./a.bend as A
import ./b.bend as B

def main() -> U32:
  U32.add(A.a(), B.b())
