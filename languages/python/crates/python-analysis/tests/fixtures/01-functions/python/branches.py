"""Step 01.09: branches and loops. Conditions are never evaluated: every arm
is possible, each arm starts from the state before the branch, and the arms
are joined after it."""


def a():
    pass


def b():
    pass


def c():
    pass


def risky():
    pass


def never_none(cond):
    f = None
    if cond:
        f = a
    else:
        f()  # f is None on this path: no edge


def both_arms(cond):
    if cond:
        f = a
    else:
        f = b
    f()


def skipped(cond):
    f = a
    if cond:
        f = b
    f()  # a, b: nodes at one site are ordered by where the function is defined


def elif_chain(x):
    if x == 1:
        f = a
    elif x == 2:
        f = b
    else:
        f = c
    f()


def loop(xs):
    f = a
    for x in xs:
        f()  # the body is walked once: a only (a known limit)
        f = b
    f()  # "didn't run", then the body: a, b


def while_loop(n):
    f = a
    while n:
        f = b
    f()


def try_except():
    f = a
    try:
        f = b
        risky()
    except ValueError:
        f()  # the handler may start before or after any body statement


# expect:
# never_none
# expect:
# both_arms
#   a
#   b
# expect:
# skipped
#   a
#   b
# expect:
# elif_chain
#   a
#   b
#   c
# expect:
# loop
#   a
#   a
#   b
# expect:
# while_loop
#   a
#   b
# expect:
# try_except
#   risky
#   a
#   b
