"""Step 04.06: split by type. build(h) with h either handler is two contexts,
one per handler class, and their objects never mix: each Job keeps the key
that matches its handler. Inline mode walks build once with both handlers,
and mixes them."""


class JsonKey:
    def encode(self):
        pass


class CsvKey:
    def encode(self):
        pass


class JsonHandler:
    def key(self):
        return JsonKey()

    def process(self, k):
        k.encode()


class CsvHandler:
    def key(self):
        return CsvKey()

    def process(self, k):
        k.encode()


class Job:
    def __init__(self, h, k):
        self.h = h
        self.k = k

    def go(self):
        self.h.process(self.k)


def build(h):
    return Job(h, h.key())


def main(c):
    h = JsonHandler() if c else CsvHandler()
    build(h).go()


# expect inline:
# main
#   JsonHandler
#   CsvHandler
#   build
#     JsonHandler.key
#       JsonKey
#     CsvHandler.key
#       CsvKey
#     Job
#   Job.go
#     JsonHandler.process
#       JsonKey.encode
#       CsvKey.encode
#     CsvHandler.process
#       JsonKey.encode
#       CsvKey.encode
# expect summary:
# main
#   JsonHandler
#   CsvHandler
#   build
#     JsonHandler.key
#       JsonKey
#     CsvHandler.key
#       CsvKey
#     Job
#   Job.go
#     JsonHandler.process
#       JsonKey.encode
#     CsvHandler.process
#       CsvKey.encode
