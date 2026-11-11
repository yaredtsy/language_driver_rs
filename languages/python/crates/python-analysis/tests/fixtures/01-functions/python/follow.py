"""Step 01.05: following a call. Each call to a project function walks the
callee's body right there, and the callee's calls become the node's
children."""

import util


def leaf():
    pass


def middle():
    leaf()


def top():
    middle()
    leaf()


def main():
    top()
    middle()


def across_files():
    util.chain()


def no_caller_scope():
    x = leaf
    reads_x()


def reads_x():
    x()  # not a local of reads_x, and no module global `x`: no edge


# expect:
# main
#   top
#     middle
#       leaf
#     leaf
#   middle
#     leaf
# expect:
# across_files
#   util.chain
#     util.helper
# expect:
# no_caller_scope
#   reads_x
