"""Step 01.07: arguments. Evaluated once at the call site, bound to the
callee's parameters as values."""

import util


def on_ok():
    pass


def on_err():
    pass


def fallback():
    pass


def run(cb):
    cb()


def run_kw(first, second):
    second()


def run_default(cb=fallback):
    cb()


def outer(cb):
    middle(cb)


def middle(cb):
    inner(cb)


def inner(cb):
    cb()


def missing(cb):
    cb()


def with_module(m):
    m.helper()


def shadow(cb):
    leak()


def leak():
    cb()  # not a parameter of leak, and no module global `cb`: no edge


def main():
    run(on_ok)
    run(on_err)
    run_kw(second=on_ok, first=on_err)
    run_default()
    run_default(on_ok)
    outer(on_err)
    missing()
    with_module(util)
    shadow(on_ok)


def read_ok():
    run(on_ok)


def read_err():
    run(on_err)


def both():
    read_ok()
    read_err()


def entry(cb):
    cb()  # an entry's parameters have no caller: Unknown, no edge


# expect:
# main
#   run
#     on_ok
#   run
#     on_err
#   run_kw
#     on_ok
#   run_default
#     fallback
#   run_default
#     on_ok
#   outer
#     middle
#       inner
#         on_err
#   missing
#   with_module
#     util.helper
#   shadow
#     leak
# expect:
# both
#   read_ok
#     run
#       on_ok
#   read_err
#     run
#       on_err
# expect:
# entry
