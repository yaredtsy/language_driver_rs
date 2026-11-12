from wrap import leaf, passthrough


class Service:
    def work(self, n):
        leaf()

    def run(self):
        passthrough(self.work)(1)
