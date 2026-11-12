from nowhere import Hidden  # no such module: the engine cannot see this class


def leaf():
    pass


class Mixin:
    @classmethod
    def find(cls):
        leaf()

    def save(self):
        leaf()


class Model(Hidden, Mixin):
    def own(self):
        leaf()
