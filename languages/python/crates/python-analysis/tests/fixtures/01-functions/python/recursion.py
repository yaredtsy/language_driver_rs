"""Step 01.06: the recursion guard. A call to a function already on the
current path is shown as a [recursive] leaf and not entered."""


def f():
    f()


def g():
    h()


def h():
    g()


def countdown(n):
    if n:
        countdown(n - 1)


def leaf():
    pass


def twice():
    leaf()
    leaf()  # seen before, but not on this path: walked again


def main():
    f()
    g()
    countdown(3)
    twice()


# expect:
# main
#   f
#     f [recursive]
#   g
#     h
#       g [recursive]
#   countdown
#     countdown [recursive]
#   twice
#     leaf
#     leaf
