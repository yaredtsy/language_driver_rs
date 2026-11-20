type Unit is Data:
  Unit{}

type Pair<A, B> is Data:
  MkPair{fst: A, snd: B}

type List<a, -A: Kind(a)> is Kind(a):
  Nil{}
  Con{head: A, tail: List<a, A>}

type Either<a, b, -A: Kind(a), -B: Kind(b)> is Kind(a <&> b):
  Left{value: A}
  Right{value: B}
