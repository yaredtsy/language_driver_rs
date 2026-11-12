"""Step 04.02, claim 2 of the plan: an object only a returned closure holds.
`make_getter` returns `get`, whose free value `b` is an object `make_getter` built. The summary must number and describe that object
too. No `# expect` block: this program is read by a unit test of `summary.rs`, not run for a tree."""


class Box:
    pass


class Handler:
    pass


def make_getter():
    b = Box()
    b.handler = Handler()

    def get():
        return b

    return get


def main():
    make_getter()
