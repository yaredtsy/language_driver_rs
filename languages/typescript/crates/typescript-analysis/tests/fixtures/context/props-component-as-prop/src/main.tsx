function star() {}
function Star() { star(); return null; }
function Card({ Icon }: { Icon: () => null }) { return <Icon />; }
function main() { return <Card Icon={Star} />; }
