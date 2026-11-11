def none():
    pass


def positional(a, b, c=3):
    pass


def positional_only(a, b, /, c):
    pass


def keyword_only(a, *, b, c=2):
    pass


def varargs(a, *args, **kwargs):
    pass


def bare_star(*, only):
    pass


def kwargs_only(**options):
    pass


def everything(a, /, b, *args, c, d=1, **kw):
    pass


def annotated(a: int, *args: str, c: float = 1.0, **kw: object) -> None:
    pass
