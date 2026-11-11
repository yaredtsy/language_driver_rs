"""Step 01.02: one call by name."""


def helper():
    pass


def other():
    pass


def unused():
    pass


def main():
    helper()
    print(f"{other()}")
    len("x")
    if True:
        helper()


# expect:
# main
#   helper
#   other
#   helper
