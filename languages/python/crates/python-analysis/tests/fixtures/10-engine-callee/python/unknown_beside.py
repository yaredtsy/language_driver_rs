from wrap import leaf, passthrough


def helper():
    leaf()


def run(flag):
    passthrough(helper if flag else missing)()
