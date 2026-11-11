"""Step 04.05: deep writes. rewire's key stops at svc.app.config (fp_depth 2),
but the object it writes is still named by its path, so the write reaches
main as an effect at Param(0).app.config.sink."""


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
    svc.app.config.sink.h.process()  # JsonHandler.process only


# expect:
# main
#   CsvHandler
#   Sink
#   Config
#   App
#   Service
#   rewire
#     JsonHandler
#   JsonHandler.process
