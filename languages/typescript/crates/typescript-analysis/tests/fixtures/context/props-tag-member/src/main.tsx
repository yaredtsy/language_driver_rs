function star() {}
function Star() { star(); return null; }
function Card(props: { Icon: () => null }) { return <props.Icon />; }
function main() { return <Card Icon={Star} />; }
