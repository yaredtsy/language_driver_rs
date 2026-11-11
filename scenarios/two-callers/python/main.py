class Writer:
    def write(self):
        return 0


class JsonWriter(Writer):
    def write(self):
        return 1


class CsvWriter(Writer):
    def write(self):
        return 2


def emit(w: Writer):
    w.write()


def read_json():
    w = JsonWriter()
    emit(w)


def read_csv():
    w = CsvWriter()
    emit(w)


def main():
    read_json()
    read_csv()
