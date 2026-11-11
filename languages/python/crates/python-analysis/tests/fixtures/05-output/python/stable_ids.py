"""Step 05.01: stable ids. The same contexts, discovered in a different
order, get the same StableContextId."""


def done():
    pass


def read_a(cb):
    cb()


def read_b(cb):
    cb()


def main():
    read_a(done)
    read_b(done)


def main2():
    read_b(done)
    read_a(done)


# expect:
# main
#   read_a
#     done
#   read_b
#     done
# expect:
# main2
#   read_b
#     done
#   read_a
#     done
