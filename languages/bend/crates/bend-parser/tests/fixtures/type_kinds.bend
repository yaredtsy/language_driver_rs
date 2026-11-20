type Vec<a, n: Nat> is Kind(a):
  Nil{}
  Cons{head: a, tail: Vec<a, n>}

type Box<-A: Data, +B: Data> is Data:
  Box{-a: A, +b: B, c}
