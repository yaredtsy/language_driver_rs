"""Step 04.08: recursion with objects. A pending object can be read
(Member), written (Write) and called; an object doesn't keep its identity
around a cycle; and effects inside a cycle reach only the exit state."""


class Handle:
    def close(self):
        pass


def walk(node):
    r = walk(node)  # pending: Apply
    r.close()  # Member, then Call
    return Handle()


def use_walk():
    walk(1)


class Report:
    def emit(self):
        pass


class Node:
    left: "Node"

    def accept(self, v: "Visitor"):
        v.visit(self)


class Visitor:
    def visit(self, node: Node):
        self.visit_node(node)

    def visit_node(self, node: Node):
        self.visit_children(node)

    def visit_children(self, node: Node):
        node.left.accept(self)  # back to accept: a cycle of four
        self.found = Report()


def visitor(root: Node):
    v = Visitor()
    root.accept(v)
    v.found.emit()


class JsonHandler:
    def process(self):
        pass


class Leaf:
    def __init__(self, h):
        self.h = h


class Branch:
    def __init__(self, child):
        self.child = child


def build(n, h):
    if n:
        return Branch(build(n - 1, h))
    return Leaf(h)


def identity_limit():
    build(1, JsonHandler()).child.h.process()  # Python: JsonHandler.process. Known limit


class Tree:
    def fill(self, n, h):
        if n:
            self.fill(n - 1, h)
            self.h.process()  # Python: h's process, set by the inner call. Known limit
        self.h = h


def tree_fill():
    Tree().fill(2, JsonHandler())


# expect inline:
# use_walk
#   walk
#     walk [recursive]
#     Handle
# expect summary:
# use_walk
#   walk
#     walk [recursive]
#     Handle.close
#     Handle
# expect:
# visitor
#   Visitor
#   Node.accept
#     Visitor.visit
#       Visitor.visit_node
#         Visitor.visit_children
#           Node.accept [recursive]
#           Report
#   Report.emit
# expect:
# identity_limit
#   JsonHandler
#   build
#     build [recursive]
#     Branch
#     Leaf
# expect:
# tree_fill
#   Tree
#   JsonHandler
#   Tree.fill
#     Tree.fill [recursive]
