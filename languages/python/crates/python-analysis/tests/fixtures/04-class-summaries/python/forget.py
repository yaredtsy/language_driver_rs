"""Step 04.03, section 3 row 2: a strong write the walk cannot say forgets what the attribute held. `wipe` writes a value of no known
type onto `p.h`, so the caller must not keep `Json` there: a read of `p.h` asks the engine's declared type (`Handler`, from the
annotation `__init__` assigns), a `Type` edge to `Handler.process`, never to `Json.process`. The forgotten attribute travels up
through `wipe_twice` as `[Unknown]` (section 4)."""

from nowhere import untyped  # the engine cannot resolve it: the walk holds `Unknown`


class Handler:
    def process(self):
        pass


class Json(Handler):
    def process(self):
        pass


class P:
    def __init__(self, h: Handler):
        self.h = h


def wipe(p):
    p.h = untyped()


def wipe_twice(p):
    wipe(p)


def main():
    p = P(Json())
    wipe(p)
    p.h.process()  # Handler.process, the declared type; not Json.process


def main2():
    p = P(Json())
    wipe_twice(p)
    p.h.process()  # the same, one call further up


# expect:
# main
#   Json
#   P
#   wipe
#   Handler.process
# expect:
# main2
#   Json
#   P
#   wipe_twice
#     wipe
#   Handler.process
