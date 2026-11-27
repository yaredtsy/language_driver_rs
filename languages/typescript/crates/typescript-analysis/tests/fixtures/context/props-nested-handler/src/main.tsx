interface Props { onClose: () => void }
function handleClose() {}
function Panel(props: Props) { const back = () => { const { onClose } = props; onClose(); }; return <button onClick={back} />; }
function main() {
    return <Panel onClose={handleClose} />;
}
