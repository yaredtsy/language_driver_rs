"""Step 04.05, claim 4: why max_path exists. last returns its argument or a
deeper one: Param(0), then Param(0).next, then Param(0).next.next, one step longer
every round of its group's fixpoint, until a path stops at the cap. The entry
builds the list itself: an entry is called with no arguments, and a parameter
would be Unknown."""


class Node:
    def __init__(self, next: "Node | None" = None):
        self.next = next


def last(n: Node):
    return last(n.next) if n.next else n


def main():
    head = Node(Node(Node()))
    last(head)


# expect:
# main
#   Node
#   Node
#   Node
#   last
#     last [recursive]
