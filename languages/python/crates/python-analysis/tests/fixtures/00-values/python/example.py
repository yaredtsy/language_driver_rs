"""The running example for 00-values: one small program, traced through every
structure from the solver down to a single value.

Entry: `main`. Nine contexts, nine walks, one reuse (`w.write(2)` reuses the
summary `p.run(1)` already made for `JsonWriter.write`).

ID: 00000000-0100-0000-0000-000000000000
"""


class JsonWriter:
    """No __init__: constructing it walks nothing.

    ID: 00000000-0101-0000-0000-000000000000
    """

    def write(self, data):
        """Uses neither parameter, so both are Unknown in its key.

        ID: 00000000-0102-0000-0000-000000000000
        """
        return None


class CsvWriter:
    """ID: 00000000-0103-0000-0000-000000000000"""

    def write(self, data):
        """ID: 00000000-0104-0000-0000-000000000000"""
        return None


class Pipeline:
    """ID: 00000000-0105-0000-0000-000000000000"""

    def __init__(self, writer):
        """Its write to self.writer is an effect on Param(0).

        ID: 00000000-0106-0000-0000-000000000000
        """
        self.writer = writer

    def run(self, data):
        """Returns Ref(Param(0).writer): the caller's own writer object.

        ID: 00000000-0107-0000-0000-000000000000
        """
        self.writer.write(data)
        return self.writer


def read_json():
    """ID: 00000000-0108-0000-0000-000000000000"""
    p = Pipeline(JsonWriter())
    w = p.run(1)
    w.write(2)


def read_csv():
    """ID: 00000000-0109-0000-0000-000000000000"""
    Pipeline(CsvWriter()).run(3)


def main():
    """The entry. ID: 00000000-010a-0000-0000-000000000000"""
    read_json()
    read_csv()
