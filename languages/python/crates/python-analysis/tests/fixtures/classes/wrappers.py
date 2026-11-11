"""The shapes the patch wrappers read: a literal, callables, a generic instance. Opened as part of the `classes` project by the tests of
`sem.rs`; nothing calls it."""

from typing import Callable


class Box[T]:
    def __init__(self, value: T):
        self.value = value


def take(
    cb: Callable[[int], str],
    any_cb: Callable[..., str],
    n: int,
    b: Box[int],
):
    cb
    any_cb
    n
    b
    three = 3
    three
