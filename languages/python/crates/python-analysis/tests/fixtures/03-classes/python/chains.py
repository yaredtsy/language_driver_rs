"""Step 03.07: chains, super() and annotations. One member lookup per hop;
super() binds the same receiver; a parameter nobody passed a value for
starts from its annotation."""


class Handler:
    def handle(self):
        pass


class LoudHandler(Handler):
    def handle(self):
        pass


class Service:
    def __init__(self, handler: Handler):
        self.handler = handler

    def call(self):
        self.handler.handle()


class Session:
    def __init__(self, service):
        self.service = service


class Base:
    def __init__(self, h):
        self.h = h


class Child(Base):
    def __init__(self, h):
        super().__init__(h)

    def go(self):
        self.h.handle()


def chain():
    s = Session(Service(LoudHandler()))
    s.service.handler.handle()


def write_then_read():
    s = Session(Service(Handler()))
    s.service.handler = LoudHandler()
    s.service.handler.handle()


def unknown_mid_chain(thing):
    thing.service.handler.handle()  # thing is Unknown: no edge, nothing guessed


def inherited():
    Child(LoudHandler()).go()


def annotated(h: Handler):
    h.handle()  # from the annotation


class A:
    def go(self):
        pass


def f(h: Handler):
    h.x = A()


def g(k: Handler):
    k.x.go()


def separate(u, v):
    f(u)  # h falls back to its annotation: an object of this run of f
    g(v)  # k is another object: f's write is not seen, no A.go


class Maker:
    @classmethod
    def build(cls):
        cls()  # cls is the class, so this constructs


class Special(Maker):
    def make(self):
        super().build()  # a classmethod through super(): cls is Special, not the object


def via_super():
    Special().make()


class H1:
    def handle(self):
        pass

class H2:
    def handle(self):
        pass

class H3:
    def handle(self):
        pass

class H4:
    def handle(self):
        pass

class H5:
    def handle(self):
        pass

class H6:
    def handle(self):
        pass

class H7:
    def handle(self):
        pass

class H8:
    def handle(self):
        pass

class H9:
    def handle(self):
        pass


class Box:
    def __init__(self, h):
        self.h = h


def fanout(c):
    if c == 9:
        b = Box(H9())
    elif c == 8:
        b = Box(H8())
    elif c == 7:
        b = Box(H7())
    elif c == 6:
        b = Box(H6())
    elif c == 5:
        b = Box(H5())
    elif c == 4:
        b = Box(H4())
    elif c == 3:
        b = Box(H3())
    elif c == 2:
        b = Box(H2())
    else:
        b = Box(H1())
    b.h.handle()  # 9 handlers, cap 8: sorted by definition, H9 is the one cut


# expect:
# chain
#   LoudHandler
#   Service
#   Session
#   LoudHandler.handle
# expect:
# write_then_read
#   Handler
#   Service
#   Session
#   LoudHandler
#   LoudHandler.handle
# expect:
# unknown_mid_chain
# expect:
# inherited
#   LoudHandler
#   Child
#     Base.__init__
#   Child.go
#     LoudHandler.handle
# expect:
# annotated
#   Handler.handle
# expect:
# Service.call
#   Handler.handle
# expect:
# separate
#   f
#     A
#   g
# expect:
# via_super
#   Special
#   Special.make
#     Maker.build
#       Special
# expect:
# fanout
#   H9
#   Box
#   H8
#   Box
#   H7
#   Box
#   H6
#   Box
#   H5
#   Box
#   H4
#   Box
#   H3
#   Box
#   H2
#   Box
#   H1
#   Box
#   H1.handle
#   H2.handle
#   H3.handle
#   H4.handle
#   H5.handle
#   H6.handle
#   H7.handle
#   H8.handle
