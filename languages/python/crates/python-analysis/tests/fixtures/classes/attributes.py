"""Attribute access: the mechanism that is not name lookup.

Everything here resolves fine with ty alone, EXCEPT `dispatch`, which is
the shape that needs a receiver you chose. Find the boundary.

FileID: 1c1c1c1c-0000-0000-0000-000000000000
"""


class Handler:
    """ID: 1c1c1c1c-1111-1111-1111-111111111111"""

    def handle(self, x):
        """ID: 1c1c1c1c-2222-2222-2222-222222222222"""
        return x


class LoudHandler(Handler):
    """ID: 1c1c1c1c-3333-3333-3333-333333333333"""

    def handle(self, x):
        """ID: 1c1c1c1c-4444-4444-4444-444444444444"""
        return str(x).upper()


class Service:
    """ID: 1c1c1c1c-5555-5555-5555-555555555555"""

    default = Handler()

    def __init__(self, handler):
        """The attribute's type is whatever the caller passed.

        ID: 1c1c1c1c-6666-6666-6666-666666666666
        """
        self.handler = handler
        self.fallback = Handler()

    def dispatch(self, payload):
        """`self.handler.handle` -- which one? Depends on the constructor
        call on this path. This is the plan/03-call-tree/07 shape.

        ID: 1c1c1c1c-7777-7777-7777-777777777777
        """
        return self.handler.handle(payload)

    def safe(self, payload):
        """`self.fallback` is assigned in __init__ from a literal
        construction, so ty can see it.

        ID: 1c1c1c1c-8888-8888-8888-888888888888
        """
        return self.fallback.handle(payload)

    def inherited(self, payload):
        """Resolved through the MRO, not through this class body.

        ID: 1c1c1c1c-9999-9999-9999-999999999999
        """
        return self.default.handle(payload)


def build_loud():
    """ID: 1c1c1c1c-aaaa-aaaa-aaaa-aaaaaaaaaaaa"""
    return Service(LoudHandler())


def build_quiet():
    """ID: 1c1c1c1c-bbbb-bbbb-bbbb-bbbbbbbbbbbb"""
    return Service(Handler())


def use_loud(payload):
    """ID: 1c1c1c1c-cccc-cccc-cccc-cccccccccccc"""
    return build_loud().dispatch(payload)


def use_quiet(payload):
    """ID: 1c1c1c1c-dddd-dddd-dddd-dddddddddddd"""
    return build_quiet().dispatch(payload)
