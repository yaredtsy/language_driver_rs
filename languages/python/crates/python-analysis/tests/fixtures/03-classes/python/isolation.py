"""Step 03.04: two instances, and objects from ty. Different origins never
share state. An object ty hands the walk (a module global, a stdlib return)
has no origin of its own, so it holds no state at all."""


def on_ok():
    pass


class JsonHandler:
    def process(self, data):
        pass


class CsvHandler:
    def process(self, data):
        pass


class FileHandler:
    def write(self):
        pass


class Pipeline:
    def __init__(self, handler):
        self.handler = handler

    def run(self, data):
        self.handler.process(data)


class Logger:
    def emit(self):
        self.handler.write()


class Greeter:
    def greet(self, cb):
        cb()


LOG_A = Logger()
LOG_B = Logger()
GREET = Greeter().greet
PIPE = Pipeline(CsvHandler())
KIND: type[Logger] = Logger  # ty: type[Logger], a SubclassOf
type H = JsonHandler


class Holder:
    handler: H  # declared through the alias


HOLDER = Holder()


def two_alive():
    a = Pipeline(JsonHandler())
    b = Pipeline(CsvHandler())
    a.run(1)
    b.run(2)


def make(h):
    return Pipeline(h)


def one_site():
    a = make(JsonHandler())
    b = make(CsvHandler())
    a.run(1)
    b.run(2)


def opaque():
    LOG_A.handler = FileHandler()  # dropped: LOG_A holds no state
    LOG_B.emit()  # must not find FileHandler.write


def global_write():
    LOG_A.handler = FileHandler()
    LOG_A.emit()  # Python: FileHandler.write. Known limit: writes to globals are dropped


def global_method():
    GREET(on_ok)  # ty's bound method, given a receiver of its own


def maybe_opaque(c):
    a = Pipeline(JsonHandler())
    if c:
        p = a
    else:
        p = PIPE
    p.handler = CsvHandler()  # p may be PIPE: a weak write, `a` keeps JsonHandler too
    a.run(1)


def subclass_of():
    KIND()  # type[Logger] lifts to Class(Logger): a construction


def via_alias():
    HOLDER.handler.process(1)  # declared through an alias: ty expands it to JsonHandler


# expect:
# two_alive
#   JsonHandler
#   Pipeline
#   CsvHandler
#   Pipeline
#   Pipeline.run
#     JsonHandler.process
#   Pipeline.run
#     CsvHandler.process
# expect:
# one_site
#   JsonHandler
#   make
#     Pipeline
#   CsvHandler
#   make
#     Pipeline
#   Pipeline.run
#     JsonHandler.process
#   Pipeline.run
#     CsvHandler.process
# expect:
# opaque
#   FileHandler
#   Logger.emit
# expect:
# global_write
#   FileHandler
#   Logger.emit
# expect:
# global_method
#   Greeter.greet
#     on_ok
# expect:
# maybe_opaque
#   JsonHandler
#   Pipeline
#   CsvHandler
#   Pipeline.run
#     JsonHandler.process
#     CsvHandler.process
# expect:
# subclass_of
#   Logger
# expect:
# via_alias
#   JsonHandler.process
