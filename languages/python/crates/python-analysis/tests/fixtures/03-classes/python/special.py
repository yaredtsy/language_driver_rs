"""Step 03.06: staticmethod, classmethod, property. The decorator decides
the receiver; a property read is a call to its getter, a write to its setter."""

from functools import cached_property


def on_ok():
    pass


class JsonHandler:
    def handle(self):
        pass


class CsvHandler:
    def handle(self):
        pass


class Registry:
    def __init__(self, h):
        self.h = h

    @staticmethod
    def make(cb):
        cb()

    @classmethod
    def create(cls, h):
        return cls(h)

    @property
    def active(self):
        return self.h

    def fire(self):
        self.active.handle()

    def get_h(self):
        return self.h

    current = property(get_h)  # a property without the decorator


def main():
    Registry.make(on_ok)  # no receiver: cb is on_ok
    r = Registry.create(JsonHandler())  # cls is Registry
    r.active.handle()  # the getter is called at `r.active`
    r.fire()
    r.make(on_ok)  # a staticmethod through an instance: still no receiver
    r.current.handle()  # property(get_h): the same property type to ty


class Box:
    def __init__(self, h):
        self._h = h

    @property
    def handler(self):
        return self._h

    @handler.setter
    def handler(self, h):
        self._h = h


def swap():
    b = Box(JsonHandler())
    b.handler = CsvHandler()  # the setter runs; nothing is stored under `handler`
    b.handler.handle()  # the getter reads `_h`: CsvHandler only


class Lazy:
    def __init__(self, h):
        self.h = h

    @cached_property
    def active(self):
        return self.h


def lazy():
    Lazy(JsonHandler()).active.handle()  # known gap: cached_property isn't followed


# expect:
# main
#   Registry.make
#     on_ok
#   JsonHandler
#   Registry.create
#     Registry
#   Registry.active
#   JsonHandler.handle
#   Registry.fire
#     Registry.active
#     JsonHandler.handle
#   Registry.make
#     on_ok
#   Registry.get_h
#   JsonHandler.handle
# expect:
# swap
#   JsonHandler
#   Box
#   CsvHandler
#   Box.handler
#   Box.handler
#   CsvHandler.handle
# expect:
# lazy
#   JsonHandler
#   Lazy
