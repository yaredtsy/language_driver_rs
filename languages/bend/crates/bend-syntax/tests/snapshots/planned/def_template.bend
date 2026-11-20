# ID: FILE

# ID: d0
def twice(~f: U32 -> U32, x: U32) -> U32: f(f(x))
# ID: d1
def replicate(-A: Data, n: Nat, +x: A) -> List<&2, A>: Nil{}
