square = lambda x: x * x


def uses_lambda(items=lambda: 0):
    squares = [x * x for x in items]
    pairs = {k: v for k, v in zip(items, items)}
    total = sum(x for x in items)
    return squares, pairs, total


alias = uses_lambda

type Alias = int

x: int = 1
