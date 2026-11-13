"""Step 04.07: the alias vector is part of the key. Two calls that pass one
object twice share a context; two that pass two objects share another; the
two are not the same context."""


class JsonHandler:
    def process(self):
        pass


class CsvHandler:
    def process(self):
        pass


class Box:
    def __init__(self, h):
        self.h = h


def set_and_read(a, b):
    a.h = JsonHandler()
    b.h.process()


def main():
    a = Box(CsvHandler())
    set_and_read(a, a)
    b = Box(CsvHandler())
    set_and_read(b, b)
    c = Box(CsvHandler())
    d = Box(CsvHandler())
    set_and_read(c, d)
    e = Box(CsvHandler())
    f = Box(CsvHandler())
    set_and_read(e, f)


# expect:
# main
#   CsvHandler
#   Box
#   set_and_read
#     JsonHandler
#     JsonHandler.process
#   CsvHandler
#   Box
#   set_and_read
#     JsonHandler
#     JsonHandler.process
#   CsvHandler
#   Box
#   CsvHandler
#   Box
#   set_and_read
#     JsonHandler
#     CsvHandler.process
#   CsvHandler
#   Box
#   CsvHandler
#   Box
#   set_and_read
#     JsonHandler
#     CsvHandler.process
