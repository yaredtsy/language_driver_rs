function close() {}
interface Props { onClose: () => void }
class Panel { constructor(props: Props) { props.onClose(); } }
function main() { return <Panel onClose={close} />; }
