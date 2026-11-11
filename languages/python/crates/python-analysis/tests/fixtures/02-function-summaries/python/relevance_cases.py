"""Step 02.04: the relevance pass, one function per rule. Read by
`relevant_names`, not by `check`: there is no tree to compare, only which
parameters the pass marks. The comment on each def is the expected answer."""

G = None


def cond_only(a, b):  # b
    if a:
        b()


def bare_condition(flag):  # none
    if flag:
        pass


def attr_in_condition(cb):  # cb: `ready` may be a property, and its getter runs
    if cb.ready:
        pass


def loop_order(cb):  # cb: found on the second pass
    for _ in range(2):
        x()  # noqa: F821 -- on the first pass x holds nothing yet
        x = cb


def aug(cb):  # cb
    x = []
    x += cb
    x()


def attr_store(cb, obj):  # cb and obj: a store is a sink for both sides
    obj.h = cb


def matched(cb):  # cb
    match cb:
        case _:
            pass


def nested_class(cb):  # cb
    class C:
        h = cb

    C.h()


def global_write(cb):  # cb
    global G
    G = cb


def splats(a, *args, k, **kw):  # args
    g(*args)  # noqa: F821
