"""Step 01.12: callbacks and decorators. A call dispatches on the callee's
value, so a function that arrived through a parameter needs no new code.
Decorators are transparent: a name bound by a def resolves to that def."""

import functools


def fetch():
    pass


def on_ok():
    pass


def on_err():
    pass


def run(task, cb):
    cb(task())


def handle_ok():
    run(fetch, on_ok)


def handle_err():
    run(fetch, on_err)


def main():
    handle_ok()
    handle_err()


def outer(cb):
    middle(cb)


def middle(cb):
    cb()


def two_levels():
    outer(on_ok)


def factory():
    return on_ok


def from_a_factory():
    run(fetch, factory())


def retry(fn):
    def wrapper(*args, **kwargs):
        return fn(*args, **kwargs)

    return wrapper


@retry
def flaky():
    on_ok()


@retry
def flaky_with(cb):
    cb()


def uses_decorated():
    flaky()
    flaky_with(on_err)


def uses_stdlib():
    p = functools.partial(run, fetch)
    p(on_ok)  # partial is stdlib: the callback is never called in the tree
    sorted([1], key=on_err)


# expect:
# main
#   handle_ok
#     run
#       fetch
#       on_ok
#   handle_err
#     run
#       fetch
#       on_err
# expect:
# two_levels
#   outer
#     middle
#       on_ok
# expect:
# from_a_factory
#   factory
#   run
#     fetch
#     on_ok
# expect:
# uses_decorated
#   flaky
#     on_ok
#   flaky_with
#     on_err
# expect:
# uses_stdlib
