"""Step 04.02: objects coming back. A returned object is the caller's own
object at a path (Ref), or one the callee built (Obj), never a copy made from
its fingerprint."""


class JsonHandler:
    def process(self):
        pass


class CsvHandler:
    def process(self):
        pass


class Box:
    def run(self):
        self.handler.process()


def ident(x):
    return x  # Ref(Param(0)): the caller's own object


def make_box(h):
    b = Box()
    b.handler = h
    return b  # Obj(0), whose handler is Ref(Param(0))


def make_json():
    b = Box()
    b.handler = JsonHandler()
    return b  # Obj(0), whose handler is Obj(1)


def identity():
    b = Box()
    b.handler = CsvHandler()
    c = ident(b)
    c.handler = JsonHandler()  # c is b: the write lands on b
    b.run()


def built_inside():
    make_json().run()


def built_with_arg():
    make_box(CsvHandler()).run()


def two_from_one_site():
    a = make_box(JsonHandler())
    b = make_box(CsvHandler())
    a.run()
    b.run()


# expect:
# identity
#   Box
#   CsvHandler
#   ident
#   JsonHandler
#   Box.run
#     JsonHandler.process
# expect:
# built_inside
#   make_json
#     Box
#     JsonHandler
#   Box.run
#     JsonHandler.process
# expect:
# built_with_arg
#   CsvHandler
#   make_box
#     Box
#   Box.run
#     CsvHandler.process
# expect:
# two_from_one_site
#   JsonHandler
#   make_box
#     Box
#   CsvHandler
#   make_box
#     Box
#   Box.run
#     JsonHandler.process
#   Box.run
#     CsvHandler.process
