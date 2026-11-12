"""E1: a function `top` with a nested `inner`, and a class `top` with a method `inner`, in one file. The outline names them `top.<locals>.inner`
and `top.inner` (occurrence 0 each); `StableDef::Path::nth` counts by the name without `<locals>`, so it says 0 and 1."""


def leaf():
    pass


def top():
    def inner():
        leaf()

    inner()


class top:
    def inner(self):
        leaf()
