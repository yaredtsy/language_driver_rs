def a() -> A -> B -> C & D | E: 0
def b() -> U32: (a + b * c - d / e % f : U32)
def c() -> Bool: (a || b && c : Bool)
def d() -> U32: (a .|. b .^. c .&. d << e >> f : U32)
def e() -> Bool: (a < b && c >= d || e <= f : U32)
def f() -> List: a <> b <> c
def g() -> String: a ++ b ++ c
def h() -> T: a <&> b <&> c
def i() -> Bool: f(a) <> g(b)
