function f(x: { m(): void; n(): void; a: { m(): void } }, k: "m" | "n") {
    x.m();
    x["n"]();
    x.a.m();
    x[k]();
}
