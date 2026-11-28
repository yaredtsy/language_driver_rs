function close() {}
function openA() {}
function openB() {}
function Dialog(props: { onClose: () => void; onOpen: () => void }) { props.onClose(); return null; }
function main() { return <div><Dialog onClose={close} onOpen={openA} /><Dialog onClose={close} onOpen={openB} /></div>; }
