"""Step 02.04: relevant parameters. A parameter that cannot affect the walk
is Unknown in the key, so callers that differ only there share one summary.
9 contexts in summary mode; 10 without the rule."""


def on_ok():
    pass


def on_err():
    pass


def note(msg):
    pass


def log_event(name, payload):
    note(name)  # payload is never read: irrelevant


def forward(cb):
    x = cb
    x()  # relevant, through the local x


def keep(cb):
    return cb  # relevant: returned


def main():
    log_event("start", on_ok)
    log_event("stop", on_err)  # the same context as the line above
    forward(on_ok)
    forward(on_err)
    keep(on_ok)()
    keep(on_err)()


# expect:
# main
#   log_event
#     note
#   log_event
#     note
#   forward
#     on_ok
#   forward
#     on_err
#   keep
#   on_ok
#   keep
#   on_err
