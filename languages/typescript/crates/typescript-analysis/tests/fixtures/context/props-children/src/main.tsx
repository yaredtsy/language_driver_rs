function run() {}
function Box(props: { children: () => void }) { props.children(); return null; }
function main() { return <Box>{run}</Box>; }
