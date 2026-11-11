"""Wire ids: `target_id` is the style's prefix and the docstring ID, by kind, and empty when there is no ID."""


class Report:
    """ID: 06000000-0001-0000-0000-000000000000"""

    def __init__(self):
        self.prepare()

    def prepare(self):
        pass


def unmarked():
    pass


def marked():
    """ID: 06000000-0002-0000-0000-000000000000"""
    unmarked()


def main():
    """ID: 06000000-0003-0000-0000-000000000000"""
    Report()
    marked()
    unmarked()
    unmarked()
