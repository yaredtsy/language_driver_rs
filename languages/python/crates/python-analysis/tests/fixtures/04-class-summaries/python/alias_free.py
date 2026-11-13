"""Step 04.07, the limit of 07 §2 decision 4: an alias between a parameter and a
closure's free value is not in the key (only top-level slots are). Pinned in
both modes, like `nested` in aliases.py."""


class JsonHandler:
    def process(self):
        pass


class CsvHandler:
    def process(self):
        pass


class Box:
    def __init__(self, h):
        self.h = h


def main():
    x = Box(CsvHandler())

    def f(a):
        a.h = JsonHandler()
        x.h.process()  # Python: JsonHandler.process (a is x)

    f(x)


# expect inline:
# main
#   CsvHandler
#   Box
#   main.f
#     JsonHandler
#     JsonHandler.process
# expect summary:
# main
#   CsvHandler
#   Box
#   main.f
#     JsonHandler
#     CsvHandler.process
