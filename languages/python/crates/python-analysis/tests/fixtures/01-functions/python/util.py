"""A module the fixtures import. Not a fixture itself: no `# expect:` block."""


def helper():
    pass


def other():
    pass


def chain():
    """Used from step 05: a call inside another file."""
    helper()
