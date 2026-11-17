function describe(name: string, body: () => void): void {
  body();
}

function it(name: string, body: () => void): void {
  body();
}

function f() {}

describe("x", () => {
  it("y", () => {
    f();
  });
});
