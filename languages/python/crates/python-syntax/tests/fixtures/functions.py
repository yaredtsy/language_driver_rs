def plain():
    return 1


async def fetch(url):
    return url


@decorator
def decorated():
    pass


@one
@two(arg=1)
async def many_decorators():
    pass


def one_liner(): ...


def with_docstring():
    """Does a thing."""
    return 2


def f_with_return_annotation(a: int, b: str = "x") -> bool:
    return True
