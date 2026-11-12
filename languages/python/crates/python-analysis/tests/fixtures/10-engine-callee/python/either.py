from wrap import leaf, passthrough


def a():
    leaf()


def b():
    pass


def run(flag):
    passthrough(a if flag else b)()
