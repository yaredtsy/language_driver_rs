"""Step 02.01: the key. A call is its function plus a fingerprint of each
argument. Inline mode walks 19 bodies for main; there are 12 distinct keys
(11 from step 02.04, which drops note's unread msg from the key)."""


def a():
    pass


def b():
    pass


def run(cb):
    cb()


def note(msg):
    pass


def same_twice():
    run(a)
    run(a)  # the same key as the line above


def different():
    run(a)
    run(b)  # a different key


def literals():
    note("x")
    note("y")  # Data(str), like "x": the same key
    note(1)  # Data(int): another key


def either(c):
    f = a if c else b
    run(f)  # one key, with a union


def main():
    same_twice()
    different()
    literals()
    either(True)


# expect:
# main
#   same_twice
#     run
#       a
#     run
#       a
#   different
#     run
#       a
#     run
#       b
#   literals
#     note
#     note
#     note
#   either
#     run
#       a
#       b
