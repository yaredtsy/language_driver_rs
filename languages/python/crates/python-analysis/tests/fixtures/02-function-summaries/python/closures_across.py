"""Step 02.07: closures across a summary. A closure leaves its walk carrying
its free values, never its activation, and enters a key the same way."""


def a():
    pass


def b():
    pass


def make(f):
    def run():
        f()

    return run


def call_it(cb):
    cb()


def wrap(f):
    return make(f)


def passed_along():
    call_it(make(a))


def two_makers():
    call_it(make(a))
    call_it(make(b))  # another key for call_it: the closure's free value differs


def two_levels():
    wrap(b)()


def outer(root):
    def walk(n):
        walk(n)  # walk's free variable `walk` is walk itself: Back(0)

    walk(root)


def chain(n):
    if n:
        r = chain(n - 1)
    else:
        r = a

    def g():
        r()

    return g


def chained():
    chain(3)()  # each round nests g one level deeper, until fp_depth


def make_walker():
    def walk(n):
        walk(n)  # a returned closure that captures itself: Back(0) in the summary

    return walk


def use_walker():
    make_walker()(1)


def with_default():
    def walk(n, cb=a):
        cb()
        walk(n)  # cb's default comes from the rebuilt closure: Context.defaults

    return walk


def use_default():
    with_default()(1)


# expect:
# passed_along
#   make
#   call_it
#     make.run
#       a
# expect:
# two_makers
#   make
#   call_it
#     make.run
#       a
#   make
#   call_it
#     make.run
#       b
# expect:
# two_levels
#   wrap
#     make
#   make.run
#     b
# expect:
# outer
#   outer.walk
#     outer.walk [recursive]
# expect inline:
# chained
#   chain
#     chain [recursive]
#   chain.g
#     a
# expect summary:
# chained
#   chain
#     chain [recursive]
#   chain.g
#     a
#     chain.g [recursive]
# expect:
# use_walker
#   make_walker
#   make_walker.walk
#     make_walker.walk [recursive]
# expect:
# use_default
#   with_default
#   with_default.walk
#     a
#     with_default.walk [recursive]
