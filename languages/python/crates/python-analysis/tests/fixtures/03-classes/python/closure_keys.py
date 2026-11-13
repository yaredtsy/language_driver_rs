"""Row 11 of the effects plan: how deep a closure's captured object is keyed. `step` captures `svc`, an object three levels from the
handler (`svc.p.handler`). When `step` is the walked function (its own run), `svc` is one of its inputs, like a parameter, and is keyed from
depth 0: the key holds the handler. When `step` is a value passed to `run_it`, `svc` sits one step below the parameter `cb`, as it always
did: `run_it`'s key holds `svc.p` cut to its class, so the call `run_it` makes to `cb` does not know the handler in summary mode (the
known limit of the depth, not row 11's)."""


class JsonHandler:
    def process(self):
        pass


class Pipeline:
    def __init__(self, handler):
        self.handler = handler


class Svc:
    def __init__(self):
        self.p = Pipeline(JsonHandler())


class Slot:
    pass


def run_it(cb):
    cb()


def direct():
    svc = Svc()

    def step():
        svc.p.handler.process()

    step()


def passed():
    svc = Svc()

    def step():
        svc.p.handler.process()

    run_it(step)



def pending(holder, n):
    # `pending(holder, n - 1)` is a call in a cycle, so its value is not known yet when `step()` is keyed: a pending value two levels below
    # the free value `holder`, which the key reads now that a called closure's free values start at depth 0. The call must wait for it.
    if n:
        holder.p.handler = pending(holder, n - 1)

    def step():
        holder.p.handler.process()

    step()
    return holder.p.handler


def pending_input():
    holder = Slot()
    holder.p = Slot()
    holder.p.handler = JsonHandler()
    pending(holder, 1)


# expect:
# direct
#   Svc
#     JsonHandler
#     Pipeline
#   direct.step
#     JsonHandler.process
# expect inline:
# passed
#   Svc
#     JsonHandler
#     Pipeline
#   run_it
#     passed.step
#       JsonHandler.process
# expect summary:
# passed
#   Svc
#     JsonHandler
#     Pipeline
#   run_it
#     passed.step
# expect:
# pending_input
#   Slot
#   Slot
#   JsonHandler
#   pending
#     pending [recursive]
#     pending.step
#       JsonHandler.process
