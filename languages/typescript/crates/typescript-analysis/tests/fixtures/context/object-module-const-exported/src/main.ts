function save() {}
export const handlers: { onSave: () => void } = { onSave: save };
export function main() {
    const { onSave } = handlers;
    onSave();
}
