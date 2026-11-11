"""Step 03.05: writes from anywhere. A write in another method, from outside
the class, through a helper function, on one path only, or on one of two
objects: one rule covers all of them."""


class JsonHandler:
    def process(self):
        pass


class CsvHandler:
    def process(self):
        pass


class Pipeline:
    def __init__(self, handler):
        self.handler = handler

    def run(self):
        self.handler.process()


class Controller:
    def setup(self):
        self.h = JsonHandler()

    def go(self):
        self.h.process()


def install(p, h):
    p.handler = h


def maybe(p, h, c):
    if c:
        p.handler = h


def outside():
    p = Pipeline(CsvHandler())
    p.handler = JsonHandler()  # one object: replaces
    p.run()


def in_another_method():
    c = Controller()
    c.setup()
    c.go()


def through_a_function():
    p = Pipeline(CsvHandler())
    install(p, JsonHandler())
    p.run()


def on_one_path(c):
    p = Pipeline(CsvHandler())
    maybe(p, JsonHandler(), c)
    p.run()  # written on one path only: both


def two_candidates(c):
    a = Pipeline(CsvHandler())
    b = Pipeline(CsvHandler())
    p = a if c else b
    p.handler = JsonHandler()  # one of two objects: a weak write to both
    a.run()


# expect:
# outside
#   CsvHandler
#   Pipeline
#   JsonHandler
#   Pipeline.run
#     JsonHandler.process
# expect:
# in_another_method
#   Controller
#   Controller.setup
#     JsonHandler
#   Controller.go
#     JsonHandler.process
# expect:
# through_a_function
#   CsvHandler
#   Pipeline
#   JsonHandler
#   install
#   Pipeline.run
#     JsonHandler.process
# expect:
# on_one_path
#   CsvHandler
#   Pipeline
#   JsonHandler
#   maybe
#   Pipeline.run
#     JsonHandler.process
#     CsvHandler.process
# expect:
# two_candidates
#   CsvHandler
#   Pipeline
#   CsvHandler
#   Pipeline
#   JsonHandler
#   Pipeline.run
#     JsonHandler.process
#     CsvHandler.process
