def leaf():
    pass


try:

    def in_try():
        leaf()

except ImportError:
    pass

with open(__file__):

    def in_with():
        leaf()
