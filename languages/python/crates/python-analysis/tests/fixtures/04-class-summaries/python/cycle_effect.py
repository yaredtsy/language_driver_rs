"""Step 04.03, section 3 row 5: a recursive call's effects are not carried until CS8. `Walker.step` writes `self.x` only in its
recursive branch; the call to itself is a replayed `Apply`, which applies no effect and counts one (`effects_in_cycle`). The caller
of `step` (`main`) still gets `step`'s own effect. Nothing wrong is drawn."""


class Handler:
    pass


class Walker:
    def step(self, n):
        if n > 0:
            self.x = Handler()  # a weak effect: written in this branch only
            self.step(n - 1)  # a call to a context that is running: its effects are not carried


def main():
    w = Walker()
    w.step(1)


# expect:
# main
#   Walker
#   Walker.step
#     Handler
#     Walker.step [recursive]
