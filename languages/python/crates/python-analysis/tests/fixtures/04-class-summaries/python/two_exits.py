"""Step 04.02, claim 1 of the plan: the end state a summary reads is the exit envs joined. `pick` returns the same object from two
`return`s that left different handlers on it. No `# expect` block: this program is read by a unit test of `summary.rs`."""


class Box:
    pass


class JsonHandler:
    pass


class CsvHandler:
    pass


def pick(c):
    b = Box()
    if c:
        b.handler = JsonHandler()
        return b
    b.handler = CsvHandler()
    return b


def main():
    pick(1)
