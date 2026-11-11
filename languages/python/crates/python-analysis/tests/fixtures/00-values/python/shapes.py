"""The five shapes an AbstractValue must tell apart. Nothing here is hard to
read -- the point is to practice naming which variant each expression becomes
BEFORE you read 08-the-value.md's answer.

ID: 00000000-0000-0000-0000-000000000000
"""


def plain():
    """A bare function object. ID: 00000000-0001-0000-0000-000000000000"""
    return None


class Thing:
    """A class object -- callable, but not yet an instance.

    ID: 00000000-0002-0000-0000-000000000000
    """

    def method(self):
        """ID: 00000000-0003-0000-0000-000000000000"""
        return None


import json  # noqa: E402  (deliberately after defs -- a module object)


def uses_all_five():
    """Five expressions, five different AbstractValue shapes. Write down which
    variant each one is -- Function / Class / Instance / BoundMethod / Module --
    before you open 08-the-value.md's answers.

    ID: 00000000-0004-0000-0000-000000000000
    """
    a = plain              # noqa: F841  -- name a function, don't call it
    b = Thing              # noqa: F841  -- name a class, don't call it
    c = Thing()            # noqa: F841  -- call the class: construct
    d = c.method           # noqa: F841  -- name a method without calling it
    e = json               # noqa: F841  -- name a module
    return a, b, c, d, e
