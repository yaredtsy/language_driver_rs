trait Writer {
    fn write(&self) -> u8;
}

struct JsonWriter {}

struct CsvWriter {}

impl Writer for JsonWriter {
    fn write(&self) -> u8 {
        1
    }
}

impl Writer for CsvWriter {
    fn write(&self) -> u8 {
        2
    }
}

fn emit<W: Writer>(w: &W) {
    w.write();
}

fn read_json() {
    let w = JsonWriter {};
    emit(&w);
}

fn read_csv() {
    let w = CsvWriter {};
    emit(&w);
}

fn main() {
    read_json();
    read_csv();
}
