function base() {}
function mine() {}
function Base() { base(); return null; }
function Mine() { mine(); return null; }
function Card(props: { Icon: typeof Base }) { return <props.Icon />; }
function main() { return <Card Icon={Mine} />; }
