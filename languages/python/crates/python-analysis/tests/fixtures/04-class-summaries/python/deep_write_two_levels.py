"""Step 04.05: a deep write through two summaries. rewire writes onto an object
past its key (Param(0).app.config.sink); outer passes its own argument on, so the
write is outer's effect too, named by outer's path to the same object. main reads
it after outer returns. outer_held passes a field of its argument instead: the
caller's path to the written object is one step longer than rewire's."""


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


class Holder:
    def __init__(self, svc: Service):
        self.svc = svc


def rewire(svc: Service):
    svc.app.config.sink.h = JsonHandler()


def outer(svc: Service):
    rewire(svc)


def outer_held(holder: Holder):
    rewire(holder.svc)


def main():
    svc = Service(App(Config(Sink(CsvHandler()))))
    outer(svc)
    svc.app.config.sink.h.process()  # JsonHandler.process only


def main_held():
    holder = Holder(Service(App(Config(Sink(CsvHandler())))))
    outer_held(holder)
    holder.svc.app.config.sink.h.process()  # JsonHandler.process only


# expect:
# main
#   CsvHandler
#   Sink
#   Config
#   App
#   Service
#   outer
#     rewire
#       JsonHandler
#   JsonHandler.process
# expect:
# main_held
#   CsvHandler
#   Sink
#   Config
#   App
#   Service
#   Holder
#   outer_held
#     rewire
#       JsonHandler
#   JsonHandler.process
