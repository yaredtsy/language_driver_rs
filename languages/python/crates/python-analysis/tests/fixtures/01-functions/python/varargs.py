"""Step 01.08: *args, **kwargs and splats. Containers are not modelled, so
these bind Unknown instead of a guessed alignment."""


def a():
    pass


def b():
    pass


def collect(*fns):
    for f in fns:
        f()


def by_name(**fns):
    fns["x"]()


def two(x, y):
    y()


def after_star(*rest, cb):
    cb()


def main():
    collect(a, b)
    by_name(x=a)
    items = [a, b]
    two(*items)
    two(a, b)
    two(**{"x": a, "y": b})
    after_star(a, cb=b)


# expect:
# main
#   collect
#   by_name
#   two
#   two
#     b
#   two
#   after_star
#     b
