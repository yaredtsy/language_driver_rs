declare function timer(cb: () => void): void;
function save() {}
function start(onDone: () => void) {
    const finish = () => {
        onDone();
    };
    timer(finish);
}
function main() {
    start(save);
}
