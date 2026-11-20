# ID: unsafe-1
@unsafe def f(x: U32) -> U32: x

# ID: unsafe-2
@ unsafe
def g(x: U32) -> U32: x

@unsafe
def h?(x: U32) -> U32: x
