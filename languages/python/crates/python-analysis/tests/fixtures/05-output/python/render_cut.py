"""Step 05.02: the render budget. With render_nodes = 3 (main counts), the
tree stops after b, and main is marked: its children are partial."""

# limits: render_nodes=3


def a():
    pass


def b():
    pass


def c():
    pass


def main():
    a()
    b()
    c()


# expect inline:
# main
#   a
#   b
#   c
# expect summary:
# main [cut]
#   a
#   b
