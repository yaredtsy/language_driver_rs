"""Step 04.03, claim 3 of the plan: a write onto a closure's free object. `swap` writes `b.h`, and `b` is a value `main` holds and `swap`
captured: its summary reports the write on `Free(0)`, and `main`, the run `swap` was defined in, applies it to its own `b`."""


class Csv:
    def process(self):
        pass


class Json:
    def process(self):
        pass


class Box:
    def __init__(self, h):
        self.h = h


def main():
    b = Box(Csv())

    def swap():
        b.h = Json()  # an effect on Free(0), strong

    swap()
    b.h.process()  # Json.process only


# expect:
# main
#   Csv
#   Box
#   main.swap
#     Json
#   Json.process
