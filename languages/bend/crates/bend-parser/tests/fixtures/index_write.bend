def a(v: Array, i: U32) -> U32: v[i]
def b(v: Array, i: U32, j: U32) -> U32: v[i][j]
def c(v: Array, i: U32) -> Array: v[i] <- 1

def f(a: Array, i: U32) -> Array:
  a[i] <- 1
  a

def g(a: Array, i: U32) -> Array:
  b = a[i] <- 2
  b
