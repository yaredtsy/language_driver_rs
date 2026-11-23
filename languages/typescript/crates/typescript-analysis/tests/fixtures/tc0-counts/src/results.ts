import { useState, make } from "fakelib";

interface Handlers { run(): void }

function project(): () => void { return () => {}; }

export function results() {
  const [open, setOpen] = useState(false);
  setOpen(!open);
  const m = make<Handlers>();
  m.run();
  const p = project();
  p();
}
