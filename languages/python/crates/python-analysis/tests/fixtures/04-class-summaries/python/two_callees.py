"""Step 04.02: a site with two callees. `h.make()` calls `A.make` or `B.make`, and each builds its own `Box` with its own handler. The
two `Box`es are different objects: the one site must not give them one origin, or each `run` would see both handlers."""


class JsonHandler:
    def process(self):
        pass


class CsvHandler:
    def process(self):
        pass


class Box:
    def run(self):
        self.handler.process()


class A:
    def make(self):
        b = Box()
        b.handler = JsonHandler()
        return b


class B:
    def make(self):
        b = Box()
        b.handler = CsvHandler()
        return b


def main(c):
    h = A() if c else B()
    h.make().run()


# Inline mode runs `Box.run` once per receiver. Summary mode keys it twice (two handlers, two contexts), and its renderer draws the two
# contexts as one node, the children of both: the tree says "the handlers are Json and Csv", the graph says which `run` has which
# (`p04_02_two_callees_each_run_sees_only_its_own_handler`).
# expect inline:
# main
#   A
#   B
#   A.make
#     Box
#     JsonHandler
#   B.make
#     Box
#     CsvHandler
#   Box.run
#     JsonHandler.process
#   Box.run
#     CsvHandler.process
# expect summary:
# main
#   A
#   B
#   A.make
#     Box
#     JsonHandler
#   B.make
#     Box
#     CsvHandler
#   Box.run
#     JsonHandler.process
#     CsvHandler.process
