interface Opts { onDone: () => void }
function a() {}
function b() {}
function open(opts: Opts) { const { onDone } = opts; onDone(); }
function main(flag: boolean) {
    open(flag ? { onDone: a } : { onDone: b });
}
