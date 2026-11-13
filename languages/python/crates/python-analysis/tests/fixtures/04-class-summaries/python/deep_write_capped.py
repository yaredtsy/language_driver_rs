"""Step 04.05: a write past max_path. With max_path = 2, the object rewire
writes (svc.app.config.sink) is past the deepest path a summary may name. The
write becomes a weak Unknown at Param(0).app.config.sink and rewire's summary
carries the DEEP_WRITE gap: counted (deep_write is 1), not marked (option B: the
tree below has no [incomplete]). main keeps CsvHandler.process, and the Unknown
the write left on sink is an unresolved alternative at the call to process."""

# limits: max_path=2


class JsonHandler:
    def process(self):
        pass


class CsvHandler:
    def process(self):
        pass


class Sink:
    def __init__(self, h):
        self.h = h


class Config:
    def __init__(self, sink: Sink):
        self.sink = sink


class App:
    def __init__(self, config: Config):
        self.config = config


class Service:
    def __init__(self, app: App):
        self.app = app


def rewire(svc: Service):
    svc.app.config.sink.h = JsonHandler()


def main():
    svc = Service(App(Config(Sink(CsvHandler()))))
    rewire(svc)
    svc.app.config.sink.h.process()


# expect inline:
# main
#   CsvHandler
#   Sink
#   Config
#   App
#   Service
#   rewire
#     JsonHandler
#   JsonHandler.process
# expect summary:
# main
#   CsvHandler
#   Sink
#   Config
#   App
#   Service
#   rewire
#     JsonHandler
#   CsvHandler.process
