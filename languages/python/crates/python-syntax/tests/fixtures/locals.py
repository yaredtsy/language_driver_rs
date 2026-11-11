def top():
    def inner():
        def innermost():
            pass

    class Local:
        def method(self):
            pass

        class Nested:
            pass

    return inner


class Outer:
    def method(self):
        def helper():
            pass

        class InMethod:
            def m(self):
                pass

    class Inner:
        def method(self):
            pass


def sibling():
    def inner():
        pass
