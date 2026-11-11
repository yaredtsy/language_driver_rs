class Plain {
  constructor(private x: number, public y: string) {}

  method(a: number) {}

  static create(): Plain {
    return new Plain(1, "a");
  }

  get value() {
    return 1;
  }

  set value(v: number) {}

  #secret() {}

  async load() {}

  *items() {}

  overload(a: string): void;
  overload(a: number): void;
  overload(a: any): void {}
}

abstract class Shape {
  abstract area(): number;
  describe() {
    function helper() {}
  }
}

export class Exported {}

export default class {}

@sealed
@tag("x")
export class Decorated {
  @log
  decorated() {}
}

class Statics {
  static {
    function inStaticBlock() {}
  }
}

function makeClass() {
  class Local {
    run() {}
  }
}
