# An entry file: an alias, a nested import and a name Base has.
import Base
import ./lib.bend as L
import ./sub/deep.bend as D

def main() -> U32:
  L.double(D.answer())
