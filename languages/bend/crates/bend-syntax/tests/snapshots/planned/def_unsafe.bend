# ID: FILE

# ID: d0
@unsafe def f(x: U32) -> U32: x
# the doc of g sits above the decorator
# ID: d1
@ unsafe
def g(x: U32) -> U32: x
# ID: d2
def h?(x: U32) -> U32: x
