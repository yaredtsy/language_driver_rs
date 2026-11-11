def identity[T](value: T) -> T:
    return value


class Box[T]:
    def get[U](self, other: U) -> T:
        pass


type Pair[A, B] = tuple[A, B]
