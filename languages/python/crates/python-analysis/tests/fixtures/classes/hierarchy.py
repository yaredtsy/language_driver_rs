"""Class shapes whose base_classes output you must match.

The diamond is the one that separates "list the bases" from "linearise
the MRO". Everything else is a naming question.

FileID: 1a1a1a1a-0000-0000-0000-000000000000
"""

from abc import ABC, abstractmethod
from typing import Generic, Protocol, TypeVar

T = TypeVar("T")


class Base:
    """ID: 1a1a1a1a-1111-1111-1111-111111111111"""

    def run(self):
        """ID: 1a1a1a1a-2222-2222-2222-222222222222"""
        return "base"


class Left(Base):
    """ID: 1a1a1a1a-3333-3333-3333-333333333333"""

    def run(self):
        """ID: 1a1a1a1a-4444-4444-4444-444444444444"""
        return "left"


class Right(Base):
    """ID: 1a1a1a1a-5555-5555-5555-555555555555"""

    def run(self):
        """ID: 1a1a1a1a-6666-6666-6666-666666666666"""
        return "right"


class Diamond(Left, Right):
    """C3 says: Diamond, Left, Right, Base, object.
    A naive depth-first walk says: Diamond, Left, Base, Right, Base, object.

    ID: 1a1a1a1a-7777-7777-7777-777777777777
    """


class Plain:
    """No explicit bases. What does `object` do here?

    ID: 1a1a1a1a-8888-8888-8888-888888888888
    """


class Box(Generic[T]):
    """A generic class. Does the base render as Generic or Generic[T]?

    ID: 1a1a1a1a-9999-9999-9999-999999999999
    """

    def __init__(self, item: T):
        """ID: 1a1a1a1a-aaaa-aaaa-aaaa-aaaaaaaaaaaa"""
        self.item = item


class IntBox(Box[int]):
    """A specialised base. Base[int] or Base?

    ID: 1a1a1a1a-bbbb-bbbb-bbbb-bbbbbbbbbbbb
    """


class Runner(Protocol):
    """A Protocol.

    ID: 1a1a1a1a-cccc-cccc-cccc-cccccccccccc
    """

    def run(self): ...


class Abstract(ABC):
    """ID: 1a1a1a1a-dddd-dddd-dddd-dddddddddddd"""

    @abstractmethod
    def run(self):
        """ID: 1a1a1a1a-eeee-eeee-eeee-eeeeeeeeeeee"""


class Outer:
    """ID: 1a1a1a1a-ffff-ffff-ffff-ffffffffffff"""

    class Inner(Base):
        """A nested class with a base. What qualified name does the
        base get, and what qualified name does Inner itself get?

        ID: 1b1b1b1b-0000-0000-0000-000000000000
        """
