"""Step 01.13: the wall. Two functions per level, three levels. Inline mode
walks 15 bodies for 7 functions, and the count doubles with every level."""


def l3a():
    pass


def l3b():
    pass


def l2a():
    l3a()
    l3b()


def l2b():
    l3a()
    l3b()


def l1a():
    l2a()
    l2b()


def l1b():
    l2a()
    l2b()


def main():
    l1a()
    l1b()


# expect:
# main
#   l1a
#     l2a
#       l3a
#       l3b
#     l2b
#       l3a
#       l3b
#   l1b
#     l2a
#       l3a
#       l3b
#     l2b
#       l3a
#       l3b
