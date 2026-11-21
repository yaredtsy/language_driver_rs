type App is Data:
  App{v: U32 -> U32, t: U32 -> U32}

def view(s: U32) -> U32:
  s

def tick(s: U32) -> U32:
  s

def run(~app: App, s: U32) -> U32:
  match app:
    case App{v, t}:
      t(s)

def main() -> U32:
  run(~App{view, tick}, 0)
