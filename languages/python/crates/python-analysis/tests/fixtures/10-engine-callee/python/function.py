from wrap import leaf, passthrough


def helper():
    leaf()


def run():
    passthrough(helper)()
