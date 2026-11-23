interface Writer { write(): void }
type Cb = () => void;
function handler() {}

export class Svc {
  private later: Cb;
  private fixed: Cb = handler;
  private moved: Cb = handler;
  constructor(private readonly w: Writer, private cb: Cb) {
    this.later = handler;
  }
  run() {
    this.cb();
    this.w.write();
    this.later();
    this.fixed();
    this.moved();
  }
  move() { this.moved = handler; }
}

export abstract class Base {
  abstract step(): void;
  go() { this.step(); }
}

export class Lonely {
  private f!: Cb;
  go() { this.f(); }
}

export function build() { return new Svc({ write() {} }, handler); }
