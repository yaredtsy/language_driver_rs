def helper():
    return 1


def countdown(n):
    if n > 0:
        countdown(n - 1)
    helper()


def main():
    countdown(3)
