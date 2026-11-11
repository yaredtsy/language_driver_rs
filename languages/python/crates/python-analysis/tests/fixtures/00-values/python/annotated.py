"""Fixtures for 05-the-walk.md's annotation fallback: where a parameter's annotation is
used (its key holds nothing for it), where it is not (a module global, which ty
already answers), and a control case where nothing resolves.

ID: 00000000-0000-0000-0000-000000000001
"""

from typing import Any


class Handler:
    """ID: 00000000-0010-0000-0000-000000000001"""

    def handle(self, x):
        """ID: 00000000-0011-0000-0000-000000000001"""
        return x


def from_untyped_dict(config: dict) -> Any:
    """`config` is a bare `dict` -- ty can tell you it's a dict, nothing
    about what's inside. No annotation to fall back to either: `Any` in,
    `Any` out. This is the CONTROL case -- there is no fallback here,
    and there shouldn't be one.

    ID: 00000000-0012-0000-0000-000000000001
    """
    return config.get("handler")


def needs_the_annotation(h: Handler):
    """`h` arrives with no default, and the caller passes something the
    interpreter's environment has no record of (see run_unresolved below).
    Plain inference gives you nothing -- but the PARAMETER is annotated
    `Handler`. That's the fallback this file is about.

    ID: 00000000-0013-0000-0000-000000000001
    """
    h.handle(1)


def run_unresolved(opaque):
    """`opaque` is itself unannotated and its value genuinely can't be
    determined (imagine it arrived over RPC). Calling
    `needs_the_annotation(opaque)` passes an UNKNOWN value into a slot that
    is annotated `Handler` on the other side.

    ID: 00000000-0014-0000-0000-000000000001
    """
    needs_the_annotation(opaque)


result: Handler = Handler()
"""A module-level variable, annotated. Not the fallback: a global is not a
parameter, so ty answers it directly, from this declaration.
ID: 00000000-0015-0000-0000-000000000001
"""


def read_result():
    """Reads a global annotated with `Handler`. ID: 00000000-0016-0000-0000-000000000001"""
    result.handle(2)
