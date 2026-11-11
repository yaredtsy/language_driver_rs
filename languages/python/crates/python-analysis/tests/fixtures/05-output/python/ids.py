"""Step 05.02: the ID filter. On the request path, a function with no `ID:`
docstring is not shown, and neither is anything under it. Values still flow
through it.

ID: 05000000-0000-0000-0000-000000000000
"""

# limits: ids=on


def deeper():
    """ID: 05000000-0003-0000-0000-000000000000"""


def hidden():
    # no ID: not shown, and deeper is not shown under it either
    deeper()


def helper():
    """ID: 05000000-0001-0000-0000-000000000000"""
    hidden()


def pick():
    # no ID: not shown, but the value it returns still flows
    return deeper


def main():
    """ID: 05000000-0004-0000-0000-000000000000"""
    helper()
    hidden()
    pick()()


# expect:
# main
#   helper
#   deeper
