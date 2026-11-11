"""Step 04.07: aliases. Two parameters holding one object must be one object
in the callee's walk. The key says which parameters are the same object, so
same() and different() are two contexts."""


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
    b.h.process()  # JsonHandler.process if a and b are one object


def same():
    x = Box(CsvHandler())
    set_and_read(x, x)


def different():
    x = Box(CsvHandler())
    y = Box(CsvHandler())
    set_and_read(x, y)


class Node:
    def link(self, other):
        self.peer = other

    def touch(self, other):
        other.h = JsonHandler()
        self.peer.h.process()  # Python: JsonHandler.process (self.peer is other)


def nested():
    a = Node()
    b = Node()
    a.link(b)
    a.touch(b)  # known limit: aliasing through an attribute is not in the key


# expect:
# same
#   CsvHandler
#   Box
#   set_and_read
#     JsonHandler
#     JsonHandler.process
# expect:
# different
#   CsvHandler
#   Box
#   CsvHandler
#   Box
#   set_and_read
#     JsonHandler
#     CsvHandler.process
# expect inline:
# nested
#   Node
#   Node
#   Node.link
#   Node.touch
#     JsonHandler
#     JsonHandler.process
# expect summary:
# nested
#   Node
#   Node
#   Node.link
#   Node.touch
#     JsonHandler
