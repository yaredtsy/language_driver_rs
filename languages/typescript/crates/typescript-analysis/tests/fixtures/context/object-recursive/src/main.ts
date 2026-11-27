interface Step { visit: () => void; parent?: Step }
function save() {}
function walk(node: Step) { const { visit } = node; visit(); walk({ parent: node, visit }); }
function main() {
    walk({ visit: save });
}
