import os


def a():
    pass


def b():
    pass


if os.environ.get("X"):

    def f():
        a()

    class Impl:
        def run(self):
            a()

else:

    def f():
        b()

    class Impl:
        def run(self):
            b()
