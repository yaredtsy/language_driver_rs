def leaf():
    pass


def top():
    def inner():
        leaf()

    inner()
