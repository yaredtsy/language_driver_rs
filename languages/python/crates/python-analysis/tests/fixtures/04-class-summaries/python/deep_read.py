"""Step 04.05, claim 3: a read below the cap. peek only reads svc.app.config.sink.h;
with max_path = 2 the object at .sink is past the cap (it holds no state), and the
read goes to the engine's declared type, as it does for an object that has a name."""

# limits: max_path=2


class JsonHandler:
    def process(self):
        pass


class Sink:
    h: JsonHandler


class Config:
    sink: Sink


class App:
    config: Config


class Service:
    app: App


def peek(svc: Service):
    svc.app.config.sink.h.process()


def main(svc: Service):
    peek(svc)


# expect:
# main
#   peek
#     JsonHandler.process
