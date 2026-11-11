"""Step 02.08: cost limits and cuts. Run with max_stack = 4. The call to mid
under main > a > b > top is past the limit: cut, marked, and not reused.
main > top is reached higher up, walked again, and complete."""

# limits: max_stack=4


def handler():
    pass


def leaf():
    return handler


def mid():
    return leaf()


def top():
    return mid()


def b():
    top()


def a():
    b()


def main():
    a()
    top()()


# expect inline:
# main
#   a
#     b
#       top
#         mid
#           leaf
#   top
#     mid
#       leaf
#   handler
# expect summary:
# main [incomplete]
#   a [incomplete]
#     b [incomplete]
#       top
#         mid
#           leaf
#   top
#     mid
#       leaf
#   handler
