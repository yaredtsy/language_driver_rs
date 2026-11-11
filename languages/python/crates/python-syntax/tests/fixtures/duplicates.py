from typing import overload

if True:
    def conditional():
        return 1
else:
    def conditional():
        return 2


@overload
def twin(a: int) -> int: ...
@overload
def twin(a: str) -> str: ...
def twin(a):
    return a


def redefined():
    pass


def redefined():
    pass
