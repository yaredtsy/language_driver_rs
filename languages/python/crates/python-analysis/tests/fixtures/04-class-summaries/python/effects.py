"""Step 04.03: effects. A write a call makes on an object its caller can see
is reported relative to the inputs, and applied by the caller. __init__'s
writes to self are effects like any other."""


class JsonHandler:
    def process(self):
        pass


class CsvHandler:
    def process(self):
        pass


class Pipeline:
    def __init__(self, handler):
        self.handler = handler  # effect: Param(0).handler = Ref(Param(1)), strong

    def run(self):
        self.handler.process()


def install(p, h):
    p.handler = h  # effect: Param(0).handler = Ref(Param(1)), strong


def maybe(p, h, c):
    if c:
        p.handler = h  # written on one path: a weak effect


def fresh(p):
    p.handler = JsonHandler()  # effect: Param(0).handler = Obj(0)


def outer_install(p, h):
    install(p, h)  # install's effect becomes outer_install's own


def ctor():
    Pipeline(JsonHandler()).run()


def installed():
    p = Pipeline(CsvHandler())
    install(p, JsonHandler())
    p.run()  # JsonHandler.process only


def maybe_installed(c):
    p = Pipeline(CsvHandler())
    maybe(p, JsonHandler(), c)
    p.run()  # both


def fresh_installed():
    p = Pipeline(CsvHandler())
    fresh(p)
    p.run()  # JsonHandler.process only


def through_two():
    p = Pipeline(CsvHandler())
    outer_install(p, JsonHandler())
    p.run()  # JsonHandler.process only


# expect:
# ctor
#   JsonHandler
#   Pipeline
#   Pipeline.run
#     JsonHandler.process
# expect:
# installed
#   CsvHandler
#   Pipeline
#   JsonHandler
#   install
#   Pipeline.run
#     JsonHandler.process
# expect:
# maybe_installed
#   CsvHandler
#   Pipeline
#   JsonHandler
#   maybe
#   Pipeline.run
#     JsonHandler.process
#     CsvHandler.process
# expect:
# fresh_installed
#   CsvHandler
#   Pipeline
#   fresh
#     JsonHandler
#   Pipeline.run
#     JsonHandler.process
# expect:
# through_two
#   CsvHandler
#   Pipeline
#   JsonHandler
#   outer_install
#     install
#   Pipeline.run
#     JsonHandler.process
