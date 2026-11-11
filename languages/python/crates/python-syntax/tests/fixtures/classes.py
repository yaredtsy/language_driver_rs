class Plain:
    pass


class WithBase(Plain, metaclass=type):
    def method(self):
        return 1

    async def amethod(self):
        return 2

    @staticmethod
    def static():
        pass

    @classmethod
    def build(cls):
        return cls()

    class Inner:
        def deep(self):
            pass

        class Innermost:
            def deepest(self):
                pass


@dataclass
class Decorated:
    x: int = 0


class Property:
    @property
    def value(self):
        return self._value

    @value.setter
    def value(self, v):
        self._value = v


class Empty: ...
