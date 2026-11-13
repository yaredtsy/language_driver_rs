"""Step 04.07: the receiver is slot 0, so `a.merge(a)` is two slots holding one
object, with no special case for `self`."""


class J:
    def run(self):
        pass


class K:
    def run(self):
        pass


class Holder:
    def __init__(self, x):
        self.x = x

    def merge(self, other):
        other.x = J()
        self.x.run()  # J.run if self and other are one object


def merged():
    a = Holder(K())
    a.merge(a)


def apart():
    a = Holder(K())
    b = Holder(K())
    a.merge(b)


# expect:
# merged
#   K
#   Holder
#   Holder.merge
#     J
#     J.run
# expect:
# apart
#   K
#   Holder
#   K
#   Holder
#   Holder.merge
#     J
#     K.run
