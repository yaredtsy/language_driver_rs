function run(cb: () => void) {
    cb();
}
function f1() {}
function f2() {}
function f3() {}
function f4() {}
function f5() {}
function f6() {}
function f7() {}
function f8() {}
function f9() {}
function pick(n: number) {
    run(n === 1 ? f1 : n === 2 ? f2 : n === 3 ? f3 : n === 4 ? f4 : n === 5 ? f5 : n === 6 ? f6 : n === 7 ? f7 : n === 8 ? f8 : f9);
}
