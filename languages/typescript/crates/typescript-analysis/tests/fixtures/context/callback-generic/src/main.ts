function each<T>(x: T, f: (x: T) => void) {
    f(x);
}
function narrow<T extends string>(x: T, f: (x: T) => void) {
    f(x);
}
class Box<T> {
    run(x: T, f: (x: T) => void) {
        f(x);
    }
}
function encode(x: string) {}
function main() {
    each("s", encode);
    narrow("s", encode);
    new Box<string>().run("s", encode);
}
