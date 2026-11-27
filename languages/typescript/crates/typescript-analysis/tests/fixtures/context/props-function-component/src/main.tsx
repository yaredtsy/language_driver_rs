interface Props { onClose: () => void }
function handleClose() {}
function Panel(props: Props) {
    const { onClose } = props;
    onClose();
    return null;
}
function main() {
    return <Panel onClose={handleClose} />;
}
