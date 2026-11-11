"""Step 01.03: locals and the env. ty agrees with every answer here; that is
the point. The env is built on code ty can check, before step 07 puts values
in it that ty can't know."""


def a():
    pass


def b():
    pass


def pick():
    return a


def alias():
    f = a
    f()


def rebinding():
    f = a
    f()
    f = b
    f()


def chained():
    f = g = b
    g()


def once():
    f = g = pick()


def walrus():
    (f := b)()
    f()


def unpack():
    f, g = a, b
    g()


# expect:
# alias
#   a
# expect:
# rebinding
#   a
#   b
# expect:
# chained
#   b
# expect:
# once
#   pick
# expect:
# walrus
#   b
#   b
# expect:
# unpack
#   b
