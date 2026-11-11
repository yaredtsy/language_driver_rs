"""Step 02.06: recursion groups. Functions that call each other form one
group, walked once each and solved together, the same from any entry."""


def done():
    pass


def num():
    pass


def binop():
    pass


# two functions
def f(n):
    return g(n)


def g(n):
    if n:
        return f(n - 1)
    return done


# three: a -> b -> c -> a
def a(n):
    if n:
        return b(n)
    return done


def b(n):
    return c(n)


def c(n):
    return a(n - 1)


# a parser: combine is first reached during the fixpoint, and joins
def parse_expr(t):
    left = parse_atom(t)
    return combine(left, t)


def parse_atom(t):
    if t:
        return parse_expr(t)
    return num


def combine(left, t):
    if t:
        parse_expr(t)
        return binop
    return left


def main():
    f(1)()
    b(2)()
    parse_expr(1)()


# a recursive call's value used in the caller of the cycle
def ping(n):
    r = pong(n)
    r()  # summary: done. Inline: r is Unknown
    return done


def pong(n):
    return ping(n - 1)


def ping_entry():
    ping(2)


# a group that turns out to be part of a bigger one
def outer(n):
    return x(n)


def x(n):
    if n:
        return y(n)
    return outer


def y(n):
    h = x(n - 1)
    return h(n)  # in the fixpoint h is outer, still running further down: merge


def merge_entry():
    outer(1)


# The parser again, with helpers first reached inside its fixpoint.
# walk_list is its own cycle: a second fixpoint runs inside the parser's.
def walk_list(xs):
    if xs:
        return walk_list(xs)
    return done


# walk_back's walk never touches the parser, but its fixpoint's replay does
def walk_back(xs):
    if xs:
        f = walk_back(xs)  # pending: walk_back is running
        r = f(xs)  # known only in walk_back's fixpoint: f is nested_expr
        r()  # summary: num, binop, once walk_back's group joins the parser's
        return f
    return nested_expr


def nested_expr(t):
    left = nested_atom(t)
    return nested_combine(left, t)


def nested_atom(t):
    if t:
        return nested_expr(t)
    return num


def nested_combine(left, t):
    if t:
        walk_list(t)
        walk_back(t)
        nested_expr(t)
        return binop
    return left


def nested_entry():
    nested_expr(1)()


# A walk that reaches the resolving group while it is parked under another
# cycle: helper inherits helper2's `joins`, instead of resolving on its own
# (where only the nested-fixpoint rule would catch it).
def up_expr(t):
    left = up_atom(t)
    return up_combine(left, t)


def up_atom(t):
    if t:
        return up_expr(t)
    return num


def up_combine(left, t):
    if t:
        helper(t)
        return binop
    return left


def helper(t):
    if t:
        return helper2(t)
    return num


def helper2(t):
    r = up_expr(t)  # the resolving group: helper2 joins it
    r()  # summary: num, binop
    return helper(t)  # helper is still running: helper2 parks under it


def up_entry():
    up_expr(1)()


# expect:
# main
#   f
#     g
#       f [recursive]
#   done
#   b
#     c
#       a
#         b [recursive]
#   done
#   parse_expr
#     parse_atom
#       parse_expr [recursive]
#     combine
#       parse_expr [recursive]
#   num
#   binop
# expect inline:
# ping_entry
#   ping
#     pong
#       ping [recursive]
# expect summary:
# ping_entry
#   ping
#     pong
#       ping [recursive]
#     done
# expect inline:
# merge_entry
#   outer
#     x
#       y
#         x [recursive]
# expect summary:
# merge_entry
#   outer
#     x
#       y
#         x [recursive]
#         outer [recursive]
# expect inline:
# nested_entry
#   nested_expr
#     nested_atom
#       nested_expr [recursive]
#     nested_combine
#       walk_list
#         walk_list [recursive]
#       walk_back
#         walk_back [recursive]
#       nested_expr [recursive]
#   num
#   binop
# expect summary:
# nested_entry
#   nested_expr
#     nested_atom
#       nested_expr [recursive]
#     nested_combine
#       walk_list
#         walk_list [recursive]
#       walk_back
#         walk_back [recursive]
#         nested_expr [recursive]
#         num
#         binop
#       nested_expr [recursive]
#   num
#   binop
# expect inline:
# up_entry
#   up_expr
#     up_atom
#       up_expr [recursive]
#     up_combine
#       helper
#         helper2
#           up_expr [recursive]
#           helper [recursive]
#   num
#   binop
# expect summary:
# up_entry
#   up_expr
#     up_atom
#       up_expr [recursive]
#     up_combine
#       helper
#         helper2
#           up_expr [recursive]
#           num
#           binop
#           helper [recursive]
#   num
#   binop
