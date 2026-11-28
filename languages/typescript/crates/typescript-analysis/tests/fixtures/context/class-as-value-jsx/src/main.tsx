function star() {}
class Star { constructor() { star(); } render() { return null; } }
function Card({ Icon }: { Icon: new () => object }) { return <Icon />; }
function main() { return <Card Icon={Star} />; }
