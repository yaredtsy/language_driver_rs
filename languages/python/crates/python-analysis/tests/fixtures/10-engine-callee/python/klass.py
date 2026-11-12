from wrap import leaf, passthrough


class Thing:
    def __init__(self):
        leaf()


def run():
    passthrough(Thing)()
