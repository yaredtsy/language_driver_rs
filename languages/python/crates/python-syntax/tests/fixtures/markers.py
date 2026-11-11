def marked():
    """ID: abc123"""


def marked_after_summary():
    """Does a thing.

    ID: 0Q8V5W4T2F7KX9M3B6D1RZ4H7K
    """


def single_quoted():
    'Summary. ID: single'


def no_colon_space():
    """ID:tight"""


def last_one_wins():
    """ID: first
    ID: second
    """


def prose_counts():
    """The text says ID: nothing-real, and nothing real below."""


def unmarked():
    """Just a summary."""


def no_docstring():
    return 1


def comment_is_not_a_marker():
    # ID: in-a-comment
    return 1


def not_the_first_statement():
    x = 1
    """ID: too-late"""
    return x


def other_label():
    """Other: not-an-id"""


def empty_value():
    """ID:"""


class MarkedClass:
    """ID: class-marker"""

    def method(self):
        """ID: method-marker"""
