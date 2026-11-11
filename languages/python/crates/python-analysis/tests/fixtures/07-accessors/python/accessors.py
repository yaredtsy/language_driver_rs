"""Property accessors: a getter and a setter are calls, made at the attribute expression, with the object as their receiver. Summary mode
makes them the way inline mode does. A deleter is modelled by neither, and `del b.h` shows a quirk both modes share: the walk reads `b.h`
to find the object, so the GETTER runs (a `Box.h` node), where Python would run the deleter. Kept as it is: the scope is what inline does."""


class Box:
    @property
    def h(self):
        return 1

    @h.setter
    def h(self, v):
        pass

    @h.deleter
    def h(self):
        pass


def reads():
    b = Box()
    b.h


def writes():
    b = Box()
    b.h = 2


def deletes():
    b = Box()
    del b.h


def both():
    b = Box()
    b.h = b.h


# expect:
# reads
#   Box
#   Box.h
# expect:
# writes
#   Box
#   Box.h
# expect:
# deletes
#   Box
#   Box.h
# expect:
# both
#   Box
#   Box.h
#   Box.h
