import Base

# Bend has no receiver to dispatch on. The nearest form is a template parameter: `emit` calls whatever `~write` its caller hands it.
def json_write(x: U32) -> U32:
  1

def csv_write(x: U32) -> U32:
  2

def emit(~write: U32 -> U32) -> U32:
  write(0)

def read_json() -> U32:
  emit(~json_write)

def read_csv() -> U32:
  emit(~csv_write)

def main() -> U32:
  U32.add(read_json(), read_csv())
