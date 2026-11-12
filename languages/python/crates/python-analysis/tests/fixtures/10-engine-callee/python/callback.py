from typing import Callable


def run(cb: Callable[[], None]):
    cb()
