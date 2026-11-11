"""Step 04.04: resolve, then write. Every value of a summary means "as it was
when the call started", so all of them are read from the state before the
call, and only then are the effects written."""


class JsonHandler:
    def process(self):
        pass


class CsvHandler:
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
