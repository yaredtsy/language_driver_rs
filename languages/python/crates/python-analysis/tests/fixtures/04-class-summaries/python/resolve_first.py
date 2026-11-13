"""Step 04.04: resolve, then write. Every value of a summary means "as it was
when the call started", so all of them are read from the state before the
call, and only then are the effects written."""


class JsonHandler:
    def process(self):
        pass


class CsvHandler:
    def process(self):
        pass


class XmlHandler:
    def process(self):
        pass


class Box:
    def __init__(self, h):
        self.h = h

    def replace(self, h):
        old = self.h
        self.h = h
        return old  # returns Ref(Param(0).h): the handler BEFORE the call


class Pair:
    def __init__(self, x, y):
        self.x = x
        self.y = y

    def swap(self):
        t = self.x
        self.x = self.y
        self.y = t  # effects: x = Ref(Param(0).y), y = Ref(Param(0).x)


class A:
    def reset(self, box):
        old = box.h
        box.h = JsonHandler()
        return old  # returns Ref(Param(1).h), writes box.h = JsonHandler


class B:
    def reset(self, box):
        old = box.h
        box.h = CsvHandler()
        return old  # returns Ref(Param(1).h), writes box.h = CsvHandler


def replacing():
    b = Box(JsonHandler())
    old = b.replace(CsvHandler())
    old.process()  # JsonHandler.process only
    b.h.process()  # CsvHandler.process only


def swapping():
    p = Pair(JsonHandler(), CsvHandler())
    p.swap()
    p.x.process()  # CsvHandler.process only
    p.y.process()  # JsonHandler.process only


def show(b, old):
    b.h.process()
    old.process()


def argument_effects():
    box = Box(JsonHandler())
    # the call in the argument runs first: its write is in the state `show`'s summary is read against
    show(box, box.replace(CsvHandler()))  # b.h is CsvHandler, old is JsonHandler


# Both modes: `old.process()` is the old handler only (each callee resolves against the one before). `box.h` after the call: inline mode joins
# the two callees' states, so it holds the two new handlers. Summary mode applies each callee's write weakly (two callees at the site: the
# write is not known to have happened), so `box.h` keeps the old handler too, and `box.h.process()` draws a second `XmlHandler.process`:
# a wrong edge, not a missing one.
def two_callees_write(c):
    box = Box(XmlHandler())
    h = A() if c else B()
    old = h.reset(box)
    old.process()  # XmlHandler.process only: both callees resolve against the one before
    box.h.process()  # JsonHandler.process and CsvHandler.process: one callee ran, which one is not known


# expect:
# replacing
#   JsonHandler
#   Box
#   CsvHandler
#   Box.replace
#   JsonHandler.process
#   CsvHandler.process
# expect:
# swapping
#   JsonHandler
#   CsvHandler
#   Pair
#   Pair.swap
#   CsvHandler.process
#   JsonHandler.process
# expect:
# argument_effects
#   JsonHandler
#   Box
#   CsvHandler
#   Box.replace
#   show
#     CsvHandler.process
#     JsonHandler.process
# expect inline:
# two_callees_write
#   XmlHandler
#   Box
#   A
#   B
#   A.reset
#     JsonHandler
#   B.reset
#     CsvHandler
#   XmlHandler.process
#   JsonHandler.process
#   CsvHandler.process
# expect summary:
# two_callees_write
#   XmlHandler
#   Box
#   A
#   B
#   A.reset
#     JsonHandler
#   B.reset
#     CsvHandler
#   XmlHandler.process
#   JsonHandler.process
#   CsvHandler.process
#   XmlHandler.process
