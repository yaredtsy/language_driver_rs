"""Step 02.05: recursion, pending values. A recursive call is recorded, not
entered. Its value is found afterwards, by repeating only the operations
that waited on it. Inline mode can't do that, so the modes differ on walk."""


def done():
    pass


def grow(n):
    if n:
        return grow(n - 1)
    return done


def walk(n):
    r = walk(n - 1)
    r()  # summary mode: done. Inline mode: r is Unknown, no edge
    return done


def rec(n):
    return rec(n - 1)  # no base case: returns nothing


def main():
    grow(3)()
    walk(3)
    rec(3)()


# expect inline:
# main
#   grow
#     grow [recursive]
#   done
#   walk
#     walk [recursive]
#   rec
#     rec [recursive]
# expect summary:
# main
#   grow
#     grow [recursive]
#   done
#   walk
#     walk [recursive]
#     done
#   rec
#     rec [recursive]
