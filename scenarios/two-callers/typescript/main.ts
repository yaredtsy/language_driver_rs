// A module, as TypeScript files are: a script would merge its functions with the DOM's globals (`fetch`).
export {};

class Writer {
  write(): number {
    return 0;
  }
}

class JsonWriter extends Writer {
  write(): number {
    return 1;
  }
}

class CsvWriter extends Writer {
  write(): number {
    return 2;
  }
}

function emit(w: Writer) {
  w.write();
}

function read_json() {
  const w = new JsonWriter();
  emit(w);
}

function read_csv() {
  const w = new CsvWriter();
  emit(w);
}

function main() {
  read_json();
  read_csv();
}
