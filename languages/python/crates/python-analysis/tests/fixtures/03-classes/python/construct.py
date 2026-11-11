"""Step 03.01: constructing. Calling a class makes a new object and walks its
__init__ with self bound to it. The node is the class; its children are
__init__'s calls."""


def log():
    pass


def setup():
    pass


class Plain:
    pass  # no __init__: the node has no children


class Pipeline:
    def __init__(self):
        setup()


class Wrapper:
    def __init__(self, inner):
        log()


class Base:
    def __init__(self):
        log()


class Child(Base):
    pass  # __init__ inherited from Base


class Tree:
    def __init__(self):
        Tree()  # constructs itself: a [recursive] leaf, keyed on the class


def make():
    return Pipeline()


def main():
    Plain()
    Pipeline()
    Wrapper(Pipeline())
    Child()
    make()
    Tree()
    dict()  # not project code: no node


# expect:
# main
#   Plain
#   Pipeline
#     setup
#   Pipeline
#     setup
#   Wrapper
#     log
#   Child
#     log
#   make
#     Pipeline
#       setup
#   Tree
#     Tree [recursive]
