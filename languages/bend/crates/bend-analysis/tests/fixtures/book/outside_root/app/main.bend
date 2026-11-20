import Base
import ./local.bend as Local
import ../shared/lib.bend as Lib

def main() -> U32:
  U32.add(Local.a(), Lib.b())
