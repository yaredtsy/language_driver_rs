"""Step 02.02: the summary. Each call's callee is walked from its key alone,
and the caller reads the callee's returns from its summary."""


def a():
    pass


def b():
    pass


def pick(c):
    if c:
        return a
    return b


def ident(f):
    return f


def chain(f):
    return ident(ident(f))


def nothing():
    pass


def main():
    pick(1)()
    chain(a)()
    ident(pick(0))()
    nothing()()


# expect:
# main
#   pick
#   a
#   b
#   chain
#     ident
#     ident
#   a
#   pick
#   ident
#   a
#   b
#   nothing
