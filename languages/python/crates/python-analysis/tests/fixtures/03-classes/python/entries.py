"""Step 03.08: entries. Every function is its own system, started like main.
The prologue walks down from the module to the entry: each enclosing function
is run (closures read its run), and a method gets an object built for it
(__init__ runs). Its effects are kept, its nodes are not shown. Callers never
matter."""


def log():
    pass


class Handler:
    def handle(self):
        pass


class JsonHandler:
    def process(self):
        pass


class A:
    def go(self):
        pass


class B:
    def go(self):
        pass


def process(h: Handler, n=3):
    h.handle()  # a function: no prologue, h from its annotation


class Pipeline:
    def __init__(self, handler):
        self.handler = handler


class Svc:
    def __init__(self):
        self.p = Pipeline(JsonHandler())  # an object graph only __init__ knows
        log()

    def run(self):
        self.p.handler.process()

    def build(self):
        def step():
            self.p.handler.process()  # `self` is a free variable of step

        return step


class Base:
    def __init__(self, h):
        self.h = h


class Child(Base):
    def __init__(self):
        super().__init__(JsonHandler())

    def go(self):
        self.h.process()


class Job:
    def __init__(self):
        self.h = A()

    def configure(self):
        self.h = B()

    def run(self):
        self.h.go()  # only __init__ is part of the start: A.go


class Reg:
    def __init__(self):
        log()

    @classmethod
    def create(cls):
        cls()  # the entry's own call: shown

    @staticmethod
    def make(cb):
        cb()  # cb is unknown: no edge


class Box:
    def __init__(self):
        self._h = JsonHandler()

    @property
    def h(self):
        self._h.process()
        return self._h


def outer():
    h = A()

    def inner():
        h.go()  # read when inner runs: after outer's last line, h is B

    h = B()


def with_param(h: Handler):
    def inner():
        h.handle()  # outer's parameter: outer ran with it from its annotation


def with_default():
    h = A()

    def inner(x=h):
        x.go()  # a default is taken when `def` runs: A, not the B after it

    h = B()


def siblings():
    def helper():
        log()

    def inner():
        helper()  # a sibling: a closure from the same run


def branches(c):
    if c:
        h = A()
    else:
        h = B()

    def inner():
        h.go()  # both arms reach the run's end: A.go and B.go


def recursive():
    def walk(n):
        log()
        walk(n)  # the entry recursing: still a [recursive] leaf


def a():
    x = A()

    def b():
        y = B()

        def c():
            x.go()  # from a's run
            y.go()  # from b's run

        return c

    return b


def make():
    h = JsonHandler()

    class Local:
        def __init__(self):
            self.h2 = h  # __init__ reads make's run too: the class is skipped

        def run(self):
            h.process()  # make's local, through Local
            self.h2.process()

    return Local


class Outer:
    def __init__(self):
        log()  # never run for Outer.Inner.m: Inner's methods can't see an Outer

    class Inner:
        def __init__(self):
            self.h = JsonHandler()

        def m(self):
            self.h.process()


class Host:
    def __init__(self):
        self.h = JsonHandler()

    def meth(self):
        host = self

        class Local:
            def m(self):
                host.h.process()  # only if Host was built and meth ran

        return Local


class Holder:
    pass


def setup(holder):
    h = A()

    def cb():
        h.go()  # called after setup returned: reads the end of setup's run

    holder.cb = cb
    h = B()  # the last statement: the run must keep it


def fire():  # not an entry case: ordinary code, the end-of-run rule
    holder = Holder()
    setup(holder)
    holder.cb()


class Tree:
    def __init__(self):
        Tree()  # recursive, inside the prologue: hidden

    def walk(self):
        log()


# expect:
# process
#   Handler.handle
# expect:
# Svc.run
#   JsonHandler.process
# expect:
# Child.go
#   JsonHandler.process
# expect:
# Job.run
#   A.go
# expect:
# Reg.create
#   Reg
#     log
# expect:
# Reg.make
# expect:
# Box.h
#   JsonHandler.process
# expect:
# outer.inner
#   B.go
# expect:
# Svc.build.step
#   JsonHandler.process
# expect:
# with_param.inner
#   Handler.handle
# expect:
# with_default.inner
#   A.go
# expect:
# siblings.inner
#   siblings.helper
#     log
# expect:
# branches.inner
#   A.go
#   B.go
# expect:
# recursive.walk
#   log
#   recursive.walk [recursive]
# expect:
# a.b.c
#   A.go
#   B.go
# expect:
# make.Local.run
#   JsonHandler.process
#   JsonHandler.process
# expect:
# Outer.Inner.m
#   JsonHandler.process
# expect:
# Host.meth.Local.m
#   JsonHandler.process
# expect:
# fire
#   Holder
#   setup
#     A
#     B
#   setup.cb
#     B.go
# expect:
# Tree.walk
#   log
