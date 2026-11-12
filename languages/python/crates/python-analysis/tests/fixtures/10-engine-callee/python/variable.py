from wrap import leaf, passthrough


def helper():
    leaf()


def run():
    g = passthrough(helper)
    g()
