"""Step 03.03: attribute state. __init__ writes self.handler; run reads it
back, on the same object. The state flows out of the constructor and into
the method call."""


class JsonHandler:
    def process(self, data):
        pass


class CsvHandler:
    def process(self, data):
        pass


class Pipeline:
    def __init__(self, handler):
        self.handler = handler

    def run(self, data):
        self.handler.process(data)


def read_json():
    Pipeline(JsonHandler()).run(1)


def read_csv():
    Pipeline(CsvHandler()).run(2)


def main():
    read_json()
    read_csv()


def make(h):
    return Pipeline(h)


def via_factory():
    make(JsonHandler()).run(1)


def either(c):
    if c:
        p = Pipeline(JsonHandler())
    else:
        p = Pipeline(CsvHandler())
    p.run(1)  # two objects, one call site: a Pipeline.run node for each


# expect:
# main
#   read_json
#     JsonHandler
#     Pipeline
#     Pipeline.run
#       JsonHandler.process
#   read_csv
#     CsvHandler
#     Pipeline
#     Pipeline.run
#       CsvHandler.process
# expect:
# via_factory
#   JsonHandler
#   make
#     Pipeline
#   Pipeline.run
#     JsonHandler.process
# expect:
# either
#   JsonHandler
#   Pipeline
#   CsvHandler
#   Pipeline
#   Pipeline.run
#     JsonHandler.process
#   Pipeline.run
#     CsvHandler.process
