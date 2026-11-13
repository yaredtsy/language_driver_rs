"""A site with a project callee and an engine callee keeps what the project callee wrote. `f` is `install` or `print`: Python makes
one of the two calls, so `install`'s write may have happened, and `p.run()` may call either handler. The write is weak at this site
(`strong_ok` is false: two alternatives), and it must not be thrown away with the engine callee's state."""


class Json:
    def process(self):
        pass


class Csv:
    def process(self):
        pass


class Pipeline:
    def __init__(self, h):
        self.handler = h

    def run(self):
        self.handler.process()


def install(p, h):
    p.handler = h


def main(c):
    p = Pipeline(Csv())
    f = install if c else print
    f(p, Json())  # install may have run: handler is Json or Csv
    p.run()


# expect:
# main
#   Csv
#   Pipeline
#   Json
#   install
#   Pipeline.run
#     Json.process
#     Csv.process
