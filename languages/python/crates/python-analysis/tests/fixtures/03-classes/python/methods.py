"""Step 03.02: methods and self. ty finds the method, through the MRO; the
walker binds its own receiver, the object it is tracking."""


def on_ok():
    pass


def on_err():
    pass


class Greeter:
    def greet(self, cb):
        cb()

    def twice(self, cb):
        self.greet(cb)
        self.greet(cb)

    @classmethod
    def make(cls, cb):
        cb()
        return cls()  # cls is the class, so this constructs


class Loud(Greeter):
    pass  # greet is inherited


class Quiet(Greeter):
    def greet(self, cb):
        pass  # greet is overridden


def main():
    g = Greeter()
    g.greet(on_ok)
    Greeter.greet(g, on_err)  # unbound: g lands on self by position
    g.twice(on_ok)
    Loud().greet(on_err)
    Quiet().greet(on_ok)
    m = g.greet
    m(on_err)  # a bound method is a value
    g.make(on_ok)  # a classmethod through an instance: cls is Greeter, not g


# expect:
# main
#   Greeter
#   Greeter.greet
#     on_ok
#   Greeter.greet
#     on_err
#   Greeter.twice
#     Greeter.greet
#       on_ok
#     Greeter.greet
#       on_ok
#   Loud
#   Greeter.greet
#     on_err
#   Quiet
#   Quiet.greet
#   Greeter.greet
#     on_err
#   Greeter.make
#     on_ok
#     Greeter
