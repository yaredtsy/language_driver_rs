"""Step 01.10: returns. A call to a project function has the value the
callee returns on this path, not ty's one answer for every caller."""


def a():
    pass


def b():
    pass


def pick_a():
    return a


def pick(c):
    if c:
        return a
    return b


def ident(f):
    return f


def through(cb):
    return ident(cb)


def nothing():
    pass


def fail():
    raise ValueError()


def log(x):
    pass


def gen():
    yield a


async def fetch():
    return a


def early(c):
    f = b
    if c:
        f = a
        return
    f()  # the path that set f = a returned: b only


def main():
    pick_a()()
    f = pick(1)
    f()
    ident(b)()
    through(a)()
    nothing()()
    log(fail())
    g = gen()
    g()


async def amain():
    (await fetch())()


# expect:
# main
#   pick_a
#   a
#   pick
#   a
#   b
#   ident
#   b
#   through
#     ident
#   a
#   nothing
#   fail
#   log
#   gen
# expect:
# amain
#   fetch
#   a
# expect:
# early
#   b
