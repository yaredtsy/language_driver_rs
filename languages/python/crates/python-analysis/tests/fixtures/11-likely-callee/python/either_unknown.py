from base import Model


def run(m: Model, flag):
    (m if flag else missing).save()
