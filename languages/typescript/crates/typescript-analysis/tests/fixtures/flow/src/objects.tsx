declare function getThing(): { run(): void };
function save() {}
function other() {}
function open(opts: { onDone: () => void }) {
    opts.onDone();
}
export function literals(x: { a: () => void }) {
    const k = "key";
    open({ onDone: save });
    const short = { save, other: other, m() {}, ...x, [k]: save };
    open(short);
}
export function members(o: { a: { b: () => void } }) {
    o.a.b();
    o["a"].b();
    o?.a?.b();
    getThing().run();
}
type Props = { onClose: () => void; nested: { deep: () => void }; opt?: () => void; extra: () => void };
export function patterns({ onClose, nested: { deep }, opt = save, ...rest }: Props) {
    onClose();
    deep();
    opt();
    open(rest);
}
export function locals(p: Props) {
    const { onClose, opt = other } = p;
    const [first] = [save];
    onClose();
    opt();
    first();
}
function Panel(props: { onClose: () => void; label?: string; children?: unknown }) {
    props.onClose();
    return null;
}
export function elements(extra: { onClose: () => void }) {
    return (
        <>
            <Panel onClose={save} label="x" />
            <Panel {...extra} label>
                text
            </Panel>
        </>
    );
}
export function children() {
    return (
        <>
            <Panel onClose={save}>
            </Panel>
            <Panel onClose={save}> </Panel>
            <Panel onClose={save}>{/* note */}</Panel>
        </>
    );
}
function restPattern(...[first]: Array<() => void>) { first(); }
function arrayPattern([first]: Array<() => void>) { first(); }
function dataPattern({ id }: { id: string }) { return id; }
