function run(cb: () => void) {
    cb();
}
function main() {
    run(() => {});
    run(function () {});
    let g = () => {};
    g = function () {};
    function h() {
        g();
    }
    run(h);
}
