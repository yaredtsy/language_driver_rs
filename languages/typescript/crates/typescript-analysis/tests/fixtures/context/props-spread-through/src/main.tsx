function close() {}
interface P { onClose: () => void }
function Inner(props: P) { props.onClose(); return null; }
function Outer(props: P) { return <Inner {...props} />; }
function main() { return <Outer onClose={close} />; }
