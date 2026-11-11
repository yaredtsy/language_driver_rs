if True:
    def in_if():
        pass
else:
    def in_else():
        pass

try:
    def in_try():
        pass
except ValueError:
    def in_except():
        pass
else:
    def in_try_else():
        pass
finally:
    def in_finally():
        pass

for _ in range(1):
    def in_for():
        pass
else:
    def in_for_else():
        pass

while False:
    def in_while():
        pass

with open("f") as fh:
    def in_with():
        pass

match 1:
    case 1:
        def in_case():
            pass
    case _:
        class InCase:
            pass


class Holder:
    if True:
        def method_in_if(self):
            pass

    try:
        class InTry:
            pass
    finally:
        pass


def function():
    if True:
        def in_function_if():
            pass
    for _ in range(1):
        class InFunctionFor:
            pass
