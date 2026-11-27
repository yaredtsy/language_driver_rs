interface Props { onClose: () => void }
function handleClose() {}
function Panel({ onClose }: Props) { onClose(); return null; }
function main() {
    return <Panel onClose={handleClose} />;
}
