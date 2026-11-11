"""Step 01.11: closures. A nested def's free variables come from the run of
the function that defined it, read when they are used. Never from the
caller."""


def a():
    pass


def b():
    pass


def pick():
    return a


def same_frame():
    f = a

    def inner():
        f()

    inner()


def make(f):
    def run():
        f()

    return run


def escaping():
    r = make(b)
    r()


def caller_has_same_name():
    f = a  # the closure must not see this f
    r = make(b)
    r()


def late():
    f = None

    def run():
        f()

    f = b
    run()  # f is read when run() runs: b


def two_runs():
    x = make(a)
    y = make(b)
    x()
    y()


def setup():
    def handler(cb=pick()):  # the default runs here, when the def runs
        cb()  # a: the value computed then. ty would say Unknown (pick is unannotated)

    handler()


def entry_closure():
    f = pick()  # a, from pick's return. ty can't know it

    def inner():
        f()  # found only if the entry's own run has its scope

    inner()


# expect:
# same_frame
#   same_frame.inner
#     a
# expect:
# escaping
#   make
#   make.run
#     b
# expect:
# caller_has_same_name
#   make
#   make.run
#     b
# expect:
# late
#   late.run
#     b
# expect:
# two_runs
#   make
#   make
#   make.run
#     a
#   make.run
#     b
# expect:
# setup
#   pick
#   setup.handler
#     a
# expect:
# entry_closure
#   pick
#   entry_closure.inner
#     a
