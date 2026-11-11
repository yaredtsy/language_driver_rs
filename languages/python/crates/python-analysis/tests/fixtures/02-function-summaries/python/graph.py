"""Step 02.03: the solver and the graph. One walk per key; the tree is
rendered from the graph of contexts. 8 contexts, 8 walks, 2 reuses."""


def a():
    pass


def b():
    pass


def run(cb):
    cb()


def read_a():
    run(a)


def read_b():
    run(b)


def twice():
    run(a)
    run(a)


def main():
    read_a()
    read_b()
    twice()


# expect:
# main
#   read_a
#     run
#       a
#   read_b
#     run
#       b
#   twice
#     run
#       a
#     run
#       a
