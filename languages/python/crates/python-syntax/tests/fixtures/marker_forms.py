def new_statement():
    return 1


def one_liner(): ...


class OneLiner: pass


def one_line_docstring(): """Summary."""


def triple_single_line():
    """Summary."""


def triple_multi_line():
    """Summary.

    Details.
    """


def single_quoted():
    'Summary.'


def implicit_concat():
    "first " "second"


def raw_docstring():
    r"""Raw \d."""


@decorated
def with_decorator():
    pass


class WithDoc:
    """Summary."""

    def method(self):
        pass

    def nested_one_liner(self): ...
